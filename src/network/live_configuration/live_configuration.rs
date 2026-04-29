// (C) Copyright IBM Corp. 2025.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::current_mode::CurrentModeOfflineReason;
use super::update_thread_worker::UpdateThreadWorker;
use super::{CurrentMode, Error, OfflineMode, Result};
use crate::errors::DeserializationError;
use crate::models::Configuration;
use crate::network::http_client::ServerClient;
use crate::network::CacheFile;
use crate::utils::{ThreadHandle, ThreadStatus, Waitable};
use crate::client::{
    RuntimeEvent, RuntimeEventKind, RuntimeEventListener, RuntimeMode, RuntimeStatus,
};
use crate::{AppConfigurationOffline, ConfigurationId, ConfigurationProvider};

/// A [`ConfigurationProvider`] that keeps the configuration updated with some
/// third-party source using an asyncronous mechanism.
pub trait LiveConfiguration: ConfigurationProvider {
    /// Utility method to know the current status of the inner thread that keeps
    /// the configuration synced with the server.
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>>;

    /// Utility method to get the current operating mode of the object.
    fn get_current_mode(&self) -> Result<CurrentMode>;

    /// Stops the live runtime thread and resets in-memory state.
    fn cleanup(&mut self) -> Result<()>;

    /// Stops the live runtime thread, resets in-memory state, and clears any
    /// SDK-managed persistent cache file.
    fn cleanup_with_cache_clear(&mut self) -> Result<()>;
}

pub(crate) struct LiveConfigurationImpl {
    /// Configuration object that will be returned to consumers. This is also the object
    /// that the thread in the backend will be updating.
    configuration: Arc<Mutex<Option<Configuration>>>,

    /// Current operation mode.
    current_mode: Waitable<CurrentMode>,

    /// Handler to the internal thread that takes care of updating the [`LiveConfigurationImpl::configuration`].
    update_thread: ThreadHandle<Result<()>>,

    /// Behaviour while the server is offline
    offline_mode: OfflineMode,

    /// Runtime listeners that mirror the Node SDK emitter-style observability surface.
    runtime_event_listeners: Arc<Mutex<Vec<RuntimeEventListener>>>,
}

impl LiveConfigurationImpl {
    fn read_persistent_cache_configuration(
        path: &PathBuf,
        environment_id: &str,
        collection_id: &str,
    ) -> Option<Configuration> {
        let contents = CacheFile::read_persistent_cache_string(path);
        if contents.is_empty() {
            return None;
        }

        serde_json::from_str::<crate::network::serialization::ConfigurationJson>(&contents)
            .map_err(|e| {
                crate::Error::DeserializationError(DeserializationError {
                    string: format!(
                        "Error deserializing Configuration from file '{}'",
                        path.display()
                    ),
                    source: e.into(),
                })
            })
            .and_then(|configuration_json| {
                Configuration::new(environment_id, collection_id, configuration_json)
                    .map_err(crate::Error::from)
            })
            .ok()
    }

    fn read_bootstrap_configuration(
        path: &PathBuf,
        environment_id: &str,
        collection_id: &str,
    ) -> Option<Configuration> {
        CacheFile::read_bootstrap_string(path)
            .and_then(|contents| {
                serde_json::from_str::<crate::network::serialization::ConfigurationJson>(&contents)
                    .map_err(|e| {
                        crate::Error::DeserializationError(DeserializationError {
                            string: format!(
                                "Error deserializing Configuration from file '{}'",
                                path.display()
                            ),
                            source: e.into(),
                        })
                    })
            })
            .and_then(|configuration_json| {
                Configuration::new(environment_id, collection_id, configuration_json)
                    .map_err(crate::Error::from)
            })
            .ok()
    }

    fn preload_configuration(
        offline_mode: &OfflineMode,
    ) -> (Option<Configuration>, Option<PathBuf>) {
        match offline_mode {
            OfflineMode::PersistentCacheFile {
                path,
                environment_id,
                collection_id,
            } => (
                Self::read_persistent_cache_configuration(path, environment_id, collection_id),
                Some(path.clone()),
            ),
            OfflineMode::BootstrapFile {
                path,
                environment_id,
                collection_id,
            } => (
                Self::read_bootstrap_configuration(path, environment_id, collection_id),
                None,
            ),
            OfflineMode::FallbackData(app_configuration_offline) => {
                (Some(app_configuration_offline.config_snapshot.clone()), None)
            }
            OfflineMode::Fail | OfflineMode::Cache => (None, None),
        }
    }

    /// Creates a new [`LiveConfigurationImpl`] object and starts a thread running an instance
    /// of [`UpdateThreadWorker`].
    pub fn new<T: ServerClient>(
        offline_mode: OfflineMode,
        server_client: T,
        configuration_id: ConfigurationId,
    ) -> Self {
        let (preloaded_configuration, persistent_cache_path) =
            Self::preload_configuration(&offline_mode);
        let configuration = Arc::new(Mutex::new(preloaded_configuration));
        let current_mode =
            Waitable::new(CurrentMode::Offline(CurrentModeOfflineReason::Initializing));
        let runtime_event_listeners = Arc::new(Mutex::new(Vec::new()));

        let worker = match persistent_cache_path {
            Some(path) => UpdateThreadWorker::new(
                server_client,
                configuration_id,
                configuration.clone(),
                current_mode.clone(),
                runtime_event_listeners.clone(),
            )
            .with_persistent_cache_file(path),
            None => UpdateThreadWorker::new(
                server_client,
                configuration_id,
                configuration.clone(),
                current_mode.clone(),
                runtime_event_listeners.clone(),
            ),
        };

        let update_thread =
            ThreadHandle::new(move |terminator_receiver| worker.run(terminator_receiver));

        Self {
            configuration,
            update_thread,
            current_mode,
            offline_mode,
            runtime_event_listeners,
        }
    }

    /// Returns the current [`Configuration`] after considering the [`CurrentMode`] and the [`OfflineMode`]
    /// configured for this object.
    fn get_configuration(&self) -> Result<Configuration> {
        // TODO: Can we return a reference instead?
        match self.current_mode.get()? {
            CurrentMode::Online => {
                match &*self.configuration.lock()? {
                    // We store the configuration retrieved from the server into the Arc<Mutex> before switching the flag to Online
                    None => unreachable!(),
                    Some(configuration) => Ok(configuration.clone()),
                }
            }
            CurrentMode::Offline(current_mode_offline_reason) => match &self.offline_mode {
                OfflineMode::Fail => Err(Error::Offline(current_mode_offline_reason.clone())),
                OfflineMode::Cache => match &*self.configuration.lock()? {
                    None => Err(Error::ConfigurationNotYetAvailable),
                    Some(configuration) => Ok(configuration.clone()),
                },
                OfflineMode::FallbackData(app_configuration_offline) => {
                    Ok(app_configuration_offline.config_snapshot.clone())
                }
                OfflineMode::PersistentCacheFile {
                    path,
                    environment_id,
                    collection_id,
                }
                | OfflineMode::BootstrapFile {
                    path,
                    environment_id,
                    collection_id,
                } => Configuration::from_file(path, environment_id, collection_id)
                    .map_err(|err| Error::UnrecoverableError(err.to_string())),
            },
            CurrentMode::Defunct(result) => match &self.offline_mode {
                OfflineMode::Fail => Err(Error::ThreadInternalError(format!(
                    "Thread finished with status: {:?}",
                    result
                ))),
                OfflineMode::Cache => match &*self.configuration.lock()? {
                    None => Err(Error::UnrecoverableError(format!(
                        "Initial configuration failed to retrieve: {:?}",
                        result
                    ))),
                    Some(configuration) => Ok(configuration.clone()),
                },
                OfflineMode::FallbackData(app_configuration_offline) => {
                    Ok(app_configuration_offline.config_snapshot.clone())
                }
                OfflineMode::PersistentCacheFile {
                    path,
                    environment_id,
                    collection_id,
                }
                | OfflineMode::BootstrapFile {
                    path,
                    environment_id,
                    collection_id,
                } => Configuration::from_file(path, environment_id, collection_id)
                    .map_err(|err| Error::UnrecoverableError(err.to_string())),
            },
        }
    }
}

impl ConfigurationProvider for LiveConfigurationImpl {
    fn get_feature_ids(&self) -> crate::Result<Vec<String>> {
        self.get_configuration()?.get_feature_ids()
    }

    fn get_feature(&self, feature_id: &str) -> crate::Result<crate::models::FeatureSnapshot> {
        self.get_configuration()?.get_feature(feature_id)
    }

    fn get_property_ids(&self) -> crate::Result<Vec<String>> {
        self.get_configuration()?.get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> crate::Result<crate::models::PropertySnapshot> {
        self.get_configuration()?.get_property(property_id)
    }

    fn get_secret_property(
        &self,
        property_id: &str,
    ) -> crate::Result<crate::models::SecretPropertySnapshot> {
        self.get_configuration()?.get_secret_property(property_id)
    }

    fn is_connected(&self) -> crate::Result<bool> {
        Ok(self.get_current_mode()? == CurrentMode::Online)
    }

    fn is_online(&self) -> crate::Result<bool> {
        self.is_connected()
    }

    fn get_runtime_status(&self) -> crate::Result<Option<RuntimeStatus>> {
        let mode = self.get_current_mode()?;
        let status = match mode {
            CurrentMode::Online => RuntimeStatus {
                is_connected: true,
                mode: Some(RuntimeMode::Online),
                offline_reason: None,
            },
            CurrentMode::Offline(reason) => RuntimeStatus {
                is_connected: false,
                mode: Some(RuntimeMode::Offline),
                offline_reason: Some(reason),
            },
            CurrentMode::Defunct(_) => RuntimeStatus {
                is_connected: false,
                mode: Some(RuntimeMode::Defunct),
                offline_reason: None,
            },
        };
        Ok(Some(status))
    }

    fn add_runtime_event_listener(&self, listener: RuntimeEventListener) -> crate::Result<()> {
        self.runtime_event_listeners.lock()?.push(listener);
        Ok(())
    }

    fn wait_until_online(&self) {
        let _ = self.current_mode.wait_for_timeout(
            CurrentMode::Online,
            Duration::from_secs(30),
        );
    }

    fn cleanup(&mut self) -> crate::Result<()> {
        LiveConfiguration::cleanup(self).map_err(crate::Error::from)
    }

    fn cleanup_with_cache_clear(&mut self) -> crate::Result<()> {
        LiveConfiguration::cleanup_with_cache_clear(self).map_err(crate::Error::from)
    }
}

impl LiveConfiguration for LiveConfigurationImpl {
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>> {
        self.update_thread.get_thread_status()
    }

    fn get_current_mode(&self) -> Result<CurrentMode> {
        Ok(self.current_mode.get()?)
    }

    fn cleanup(&mut self) -> Result<()> {
        match self.update_thread.shutdown(Duration::from_secs(5)) {
            Ok(_) => {}
            Err(err) => {
                return Err(Error::UnrecoverableError(format!(
                    "Failed to stop live configuration worker: {err}"
                )));
            }
        }

        self.current_mode
            .set(CurrentMode::Defunct(Ok(())))
            .map_err(Error::from)?;
        let mut configuration = self.configuration.lock()?;
        *configuration = None;
        Ok(())
    }

    fn cleanup_with_cache_clear(&mut self) -> Result<()> {
        LiveConfiguration::cleanup(self)?;
        if let OfflineMode::PersistentCacheFile { path, .. } = &self.offline_mode {
            CacheFile::delete_file_data(path);
        }
        Ok(())
    }
}

impl LiveConfigurationImpl {
    pub(crate) fn emit_runtime_event(&self, kind: RuntimeEventKind, status: RuntimeStatus) {
        let listeners = match self.runtime_event_listeners.lock() {
            Ok(listeners) => listeners.clone(),
            Err(_) => return,
        };

        let event = RuntimeEvent { kind, status };
        for listener in listeners {
            listener(event.clone());
        }
    }
}

impl Drop for LiveConfigurationImpl {
    fn drop(&mut self) {
        let _ = LiveConfiguration::cleanup(self);
    }
}

#[cfg(test)]
mod tests {

    use std::sync::mpsc::{self, RecvError};

    use rstest::rstest;

    use crate::network::serialization::fixtures::{
        configuration_property1_enabled, example_configuration_enterprise_path,
    };

    use crate::network::http_client::WebsocketReader;
    use crate::network::live_configuration::update_thread_worker::SERVER_HEARTBEAT;
    use crate::network::NetworkResult;
    use crate::AppConfigurationOffline;

    use super::*;

    #[test]
    fn test_happy_path() {
        struct WebsocketReaderMock {
            rx: mpsc::Receiver<tungstenite::Message>,
            tx: mpsc::Sender<()>,
        }
        impl WebsocketReader for WebsocketReaderMock {
            fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
                self.tx.send(()).unwrap();
                Ok(self.rx.recv().unwrap())
            }
        }
        struct ServerClientMock {
            rx: mpsc::Receiver<Configuration>,
            websocket_rx: mpsc::Receiver<WebsocketReaderMock>,
        }
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Ok(self.rx.recv().unwrap())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                Ok(self.websocket_rx.recv().unwrap())
            }
        }

        let (websocket_factory_tx, websocket_factory_rx) = mpsc::channel();
        let (get_configuration_tx, get_configuration_rx) = mpsc::channel();
        let server_client = ServerClientMock {
            rx: get_configuration_rx,
            websocket_rx: websocket_factory_rx,
        };

        let configuration_id =
            crate::ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let mut live_config =
            LiveConfigurationImpl::new(OfflineMode::Fail, server_client, configuration_id);

        {
            // Blocked beginning of get_configuration_from_server()
            // Expect we are in initializing state (no config)
            let config = live_config.get_configuration();
            assert!(
                matches!(
                    config,
                    Err(Error::Offline(CurrentModeOfflineReason::Initializing))
                ),
                "{:?}",
                config
            );
            let thread_state = live_config.get_thread_status();
            assert!(matches!(thread_state, ThreadStatus::Running));
            let current_mode = live_config.get_current_mode();
            assert!(matches!(
                current_mode,
                Ok(CurrentMode::Offline(CurrentModeOfflineReason::Initializing))
            ));
        }

        let (read_msg_tx, read_msg_rx) = mpsc::channel();
        let (read_msg_ping_tx, read_msg_ping_rx) = mpsc::channel();
        let configuration =
            crate::network::serialization::fixtures::configuration_feature1_enabled();
        let config = {
            // allow thread to start (unblock)
            get_configuration_tx.send(configuration).unwrap();
            websocket_factory_tx
                .send(WebsocketReaderMock {
                    rx: read_msg_rx,
                    tx: read_msg_ping_tx,
                })
                .unwrap();

            // Now live_config should eventually transition into
            // online state. We can wait for this transition to happen:
            live_config.wait_until_online();
            // And assert that we are really online
            let current_mode = live_config.get_current_mode();
            assert!(matches!(current_mode, Ok(CurrentMode::Online)));
            // The initialization should have reached the first websocket read.
            // This can race slightly with the mode transition, so wait briefly
            // instead of assuming the signal is already queued.
            read_msg_ping_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            // Blocked in socket.read_msg()
            // Expect, we get a configuration and are Online / Running state
            let config_result = live_config.get_configuration();
            assert!(matches!(config_result, Ok(_)), "{:?}", config_result);
            let thread_state = live_config.get_thread_status();
            assert!(matches!(thread_state, ThreadStatus::Running));
            let current_mode = live_config.get_current_mode();
            assert!(matches!(current_mode, Ok(CurrentMode::Online)));
            config_result.unwrap()
        };

        {
            // Send a heartbeat via the websocket.
            read_msg_tx
                .send(tungstenite::Message::text(SERVER_HEARTBEAT))
                .unwrap();
            // Wait for thread to do some work and then to wait on websocket
            read_msg_ping_rx.recv().unwrap();

            // Expect no change due to heartbeat:
            let config_result = live_config.get_configuration();
            assert!(config_result.is_ok());
            assert_eq!(config_result.unwrap(), config);
            let thread_state = live_config.get_thread_status();
            assert!(matches!(thread_state, ThreadStatus::Running));
            let current_mode = live_config.get_current_mode();
            assert!(matches!(current_mode, Ok(CurrentMode::Online)));
        }

        {
            // Send any message via the websocket (it will be interpreted as new config is available).
            read_msg_tx.send(tungstenite::Message::text("")).unwrap();
            // Send the new configuration
            let configuration = configuration_property1_enabled();
            get_configuration_tx.send(configuration).unwrap();
            // Wait for thread to do some work and then to wait on websocket
            read_msg_ping_rx.recv().unwrap();

            // Expect new configuration, and still running/online
            let config_result = live_config.get_configuration();
            assert!(config_result.is_ok());
            assert_ne!(config_result.unwrap(), config);
            let thread_state = live_config.get_thread_status();
            assert!(matches!(thread_state, ThreadStatus::Running));
            let current_mode = live_config.get_current_mode();
            assert!(matches!(current_mode, Ok(CurrentMode::Online)));
        }

        {
            // After all those operations live_config should still be online.
            // `wait_until_online` should not block:
            live_config.wait_until_online();
        }

        {
            // When the client is dropped, the thread will be finished
            drop(live_config);

            read_msg_tx
                .send(tungstenite::Message::text(SERVER_HEARTBEAT))
                .unwrap();

            let r = read_msg_ping_rx.recv();
            assert!(matches!(r, Err(RecvError { .. })), "{:?}", r)
        }
    }

    // Check the configuration that is returned when CurrentMode::Online
    #[rstest]
    fn test_get_configuration_when_online(
        example_configuration_enterprise_path: std::path::PathBuf,
    ) {
        let (tx, _) = std::sync::mpsc::channel();
        let mut cfg = LiveConfigurationImpl {
            configuration: Arc::new(Mutex::new(Some(Configuration::default()))),
            offline_mode: OfflineMode::Fail,
            current_mode: Waitable::new(CurrentMode::Online),
            update_thread: ThreadHandle {
                _thread_termination_sender: tx,
                thread_handle: None,
                finished_thread_status_cached: None,
            },
            runtime_event_listeners: Arc::new(Mutex::new(Vec::new())),
        };

        {
            cfg.offline_mode = OfflineMode::Cache;
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert!(r.unwrap().features.is_empty());
        }

        {
            cfg.offline_mode = OfflineMode::Fail;
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert!(r.unwrap().features.is_empty());
        }

        {
            let offline = AppConfigurationOffline::new(
                &example_configuration_enterprise_path,
                "dev",
                "blue-charge",
            )
            .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert!(r.unwrap().features.is_empty());
        }
    }

    #[rstest]
    fn test_get_configuration_when_offline(
        example_configuration_enterprise_path: std::path::PathBuf,
    ) {
        let (tx, _) = std::sync::mpsc::channel();
        let mut cfg = LiveConfigurationImpl {
            offline_mode: OfflineMode::Fail,
            configuration: Arc::new(Mutex::new(Some(Configuration::default()))),
            current_mode: Waitable::new(CurrentMode::Offline(
                CurrentModeOfflineReason::WebsocketClosed,
            )),
            update_thread: ThreadHandle {
                _thread_termination_sender: tx,
                thread_handle: None,
                finished_thread_status_cached: None,
            },
            runtime_event_listeners: Arc::new(Mutex::new(Vec::new())),
        };

        {
            cfg.offline_mode = OfflineMode::Fail;
            let r = cfg.get_configuration();
            assert!(r.is_err(), "Error: {}", r.unwrap_err());
            assert_eq!(
                r.unwrap_err(),
                Error::Offline(CurrentModeOfflineReason::WebsocketClosed)
            );
        }

        {
            cfg.offline_mode = OfflineMode::Cache;
            {
                cfg.configuration = Arc::new(Mutex::new(None));
                let r = cfg.get_configuration();
                assert!(r.is_err());
                assert_eq!(r.unwrap_err(), Error::ConfigurationNotYetAvailable);
            }
            {
                cfg.configuration = Arc::new(Mutex::new(Some(Configuration::default())));
                let r = cfg.get_configuration();
                assert!(r.is_ok(), "Error: {}", r.unwrap_err());
                assert!(r.unwrap().features.is_empty());
            }
        }

        {
            let offline = AppConfigurationOffline::new(
                &example_configuration_enterprise_path,
                "dev",
                "blue-charge",
            )
            .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert_eq!(r.unwrap().features.len(), 5);
        }
    }

    #[rstest]
    fn test_get_configuration_when_defunct(
        example_configuration_enterprise_path: std::path::PathBuf,
    ) {
        let (tx, _) = std::sync::mpsc::channel();
        let mut cfg = LiveConfigurationImpl {
            offline_mode: OfflineMode::Fail,
            configuration: Arc::new(Mutex::new(Some(Configuration::default()))),
            current_mode: Waitable::new(CurrentMode::Defunct(Ok(()))),
            update_thread: ThreadHandle {
                _thread_termination_sender: tx,
                thread_handle: None,
                finished_thread_status_cached: None,
            },
            runtime_event_listeners: Arc::new(Mutex::new(Vec::new())),
        };

        {
            cfg.offline_mode = OfflineMode::Fail;
            let r = cfg.get_configuration();
            assert!(r.is_err(), "Error: {}", r.unwrap_err());
            assert_eq!(
                r.unwrap_err(),
                Error::ThreadInternalError("Thread finished with status: Ok(())".to_string())
            );
        }

        {
            cfg.offline_mode = OfflineMode::Cache;
            {
                cfg.configuration = Arc::new(Mutex::new(None));
                let r = cfg.get_configuration();
                assert!(r.is_err());
                assert_eq!(
                    r.unwrap_err(),
                    Error::UnrecoverableError(
                        "Initial configuration failed to retrieve: Ok(())".to_string()
                    )
                );
            }
            {
                cfg.configuration = Arc::new(Mutex::new(Some(Configuration::default())));
                let r = cfg.get_configuration();
                assert!(r.is_ok(), "Error: {}", r.unwrap_err());
                assert!(r.unwrap().features.is_empty());
            }
        }

        {
            let offline = AppConfigurationOffline::new(
                &example_configuration_enterprise_path,
                "dev",
                "blue-charge",
            )
            .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert_eq!(r.unwrap().features.len(), 5);
        }
    }
}

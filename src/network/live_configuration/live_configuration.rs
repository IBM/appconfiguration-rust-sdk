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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::current_mode::CurrentModeOfflineReason;
use super::update_thread_worker::UpdateThreadWorker;
use super::{CurrentMode, Error, OfflineMode, Result};
use crate::client::{RuntimeEventListener, RuntimeMode, RuntimeStatus};
use crate::errors::DeserializationError;
use crate::models::Configuration;
use crate::network::CacheFile;
use crate::network::http_client::ServerClient;
use crate::utils::{ThreadHandle, ThreadStatus, Waitable};
use crate::{ConfigurationId, ConfigurationProvider};

/// A [`ConfigurationProvider`] that keeps the configuration updated with some
/// third-party source using an asyncronous mechanism.
pub trait LiveConfiguration: ConfigurationProvider {
    /// Utility method to know the current status of the inner thread that keeps
    /// the configuration synced with the server.
    #[allow(dead_code)]
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>>;

    /// Utility method to get the current operating mode of the object.
    fn get_current_mode(&self) -> Result<CurrentMode>;

    /// Stops the live runtime thread and resets in-memory state.
    fn clean_up(&mut self) -> Result<()>;

    /// Stops the live runtime thread, resets in-memory state, and clears any
    /// SDK-managed persistent cache file.
    fn clean_up_with_cache_clear(&mut self) -> Result<()>;
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

    runtime_event_listeners: Arc<Mutex<Vec<RuntimeEventListener>>>,
}

impl std::fmt::Debug for LiveConfigurationImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveConfigurationImpl")
            .field("configuration", &self.configuration)
            .field("current_mode", &self.current_mode)
            .field("update_thread", &self.update_thread)
            .field("offline_mode", &self.offline_mode)
            .field(
                "runtime_event_listeners",
                &format!(
                    "[{} listeners]",
                    self.runtime_event_listeners
                        .lock()
                        .map(|l| l.len())
                        .unwrap_or(0)
                ),
            )
            .finish()
    }
}

impl LiveConfigurationImpl {
    /// Creates a new [`LiveConfigurationImpl`] object and starts a thread running an instance
    /// of [`UpdateThreadWorker`].
    ///

    pub fn new<T: ServerClient>(
        offline_mode: OfflineMode,
        server_client: T,
        configuration_id: ConfigurationId,
        initial_listeners: Vec<RuntimeEventListener>,
    ) -> Self {
        let (preloaded_configuration, persistent_cache_path) =
            Self::preload_configuration(&offline_mode);
        let configuration = Arc::new(Mutex::new(preloaded_configuration));
        let runtime_event_listeners = Arc::new(Mutex::new(initial_listeners));

        if matches!(offline_mode, OfflineMode::FallbackData(_)) {
            let current_mode = Waitable::new(CurrentMode::Defunct(Ok(())));
            let update_thread = ThreadHandle::new(move |_| Ok(()));
            drop(server_client);
            return Self {
                configuration,
                update_thread,
                current_mode,
                offline_mode,
                runtime_event_listeners,
            };
        }

        let current_mode =
            Waitable::new(CurrentMode::Offline(CurrentModeOfflineReason::Initializing));
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

    fn read_persistent_cache_configuration(
        path: &Path,
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
            OfflineMode::FallbackData(app_configuration_offline) => (
                Some(app_configuration_offline.config_snapshot.clone()),
                None,
            ),
            OfflineMode::Fail | OfflineMode::Cache => (None, None),
        }
    }

    /// Returns the current [`Configuration`] after considering the [`CurrentMode`] and the [`OfflineMode`]
    /// configured for this object.
    ///
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
            CurrentMode::Offline(current_mode_offline_reason) => {
                // Priority 1: always try the in-memory cache first — the background thread
                // preserves the last successful fetch across reconnect cycles.
                if let Some(configuration) = &*self.configuration.lock()? {
                    log::debug!(
                        "[OFFLINE] Serving stale in-memory config while reconnecting (reason: {})",
                        current_mode_offline_reason
                    );
                    return Ok(configuration.clone());
                }

                // Priority 2: no in-memory config yet — fall back to the configured strategy.
                match &self.offline_mode {
                    OfflineMode::Fail => Err(Error::Offline(current_mode_offline_reason)),
                    OfflineMode::Cache => Err(Error::ConfigurationNotYetAvailable),
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
                }
            }
            CurrentMode::Defunct(result) => {
                // Same strategy: serve stale in-memory config when the thread has exited
                // (e.g. after clean_up()) but a valid configuration is still held in memory.
                if let Some(configuration) = &*self.configuration.lock()? {
                    log::debug!(
                        "[DEFUNCT] Serving stale in-memory config (thread result: {:?})",
                        result
                    );
                    return Ok(configuration.clone());
                }

                match &self.offline_mode {
                    OfflineMode::Fail => Err(Error::ThreadInternalError(format!(
                        "Thread finished with status: {:?}",
                        result
                    ))),
                    OfflineMode::Cache => Err(Error::UnrecoverableError(format!(
                        "Initial configuration failed to retrieve: {:?}",
                        result
                    ))),
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
                }
            }
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

    fn is_online(&self) -> crate::Result<bool> {
        Ok(self.get_current_mode()? == CurrentMode::Online)
    }

    fn wait_until_online(&self) -> bool {
        if let Ok(CurrentMode::Defunct(Ok(()))) = self.current_mode.get() {
            if let OfflineMode::FallbackData(_) = &self.offline_mode {
                return true;
            }
        }
        self.current_mode
            .wait_for_timeout(CurrentMode::Online, Duration::from_secs(30))
            .is_ok()
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

    fn clean_up(&mut self) -> crate::Result<()> {
        LiveConfiguration::clean_up(self).map_err(crate::Error::from)
    }

    fn clean_up_with_cache_clear(&mut self) -> crate::Result<()> {
        LiveConfiguration::clean_up_with_cache_clear(self).map_err(crate::Error::from)
    }
}

impl LiveConfiguration for LiveConfigurationImpl {
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>> {
        self.update_thread.get_thread_status()
    }

    fn get_current_mode(&self) -> Result<CurrentMode> {
        Ok(self.current_mode.get()?)
    }

    fn clean_up(&mut self) -> Result<()> {
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

    fn clean_up_with_cache_clear(&mut self) -> Result<()> {
        LiveConfiguration::clean_up(self)?;
        if let OfflineMode::PersistentCacheFile { path, .. } = &self.offline_mode {
            CacheFile::delete_file_data(path);
        }
        Ok(())
    }
}

impl Drop for LiveConfigurationImpl {
    fn drop(&mut self) {
        let _ = LiveConfiguration::clean_up(self);
    }
}

#[cfg(test)]
mod tests {

    use std::sync::mpsc::{self, RecvError};

    use rstest::rstest;

    use crate::network::serialization::fixtures::example_configuration_enterprise_path;

    use crate::AppConfigurationOffline;
    use crate::network::NetworkError::ProtocolError;
    use crate::network::NetworkResult;
    use crate::network::http_client::WebsocketReader;
    use crate::network::live_configuration::update_thread_worker::SERVER_HEARTBEAT;
    use crate::network::serialization::ConfigurationJson;

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
            fn flush_socket(&mut self) -> tungstenite::error::Result<()> {
                Ok(())
            }
        }
        struct ServerClientMock {
            rx: mpsc::Receiver<ConfigurationJson>,
            websocket_rx: mpsc::Receiver<WebsocketReaderMock>,
        }
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Err(ProtocolError)
            }

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<ConfigurationJson> {
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
            crate::ConfigurationId::new("".into(), "environment_id".into(), "collection_id".into());
        let mut live_config =
            LiveConfigurationImpl::new(OfflineMode::Fail, server_client, configuration_id, vec![]);

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
            crate::network::serialization::fixtures::configuration_json_feature1_enabled();
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
            // The initialization should have received one message from WS.
            // We consume it. Note the `try_` here. We should not block here, as this was already asserted via the `wait_until_online()` earlier.
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
            let configuration =
                crate::network::serialization::fixtures::configuration_json_property1_enabled();
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

        // OfflineMode::Fail WITH a previously-fetched in-memory cache → serve stale config

        {
            cfg.offline_mode = OfflineMode::Fail;
            cfg.configuration = Arc::new(Mutex::new(Some(Configuration::default())));
            let r = cfg.get_configuration();
            assert!(
                r.is_ok(),
                "OfflineMode::Fail must serve stale in-memory config while reconnecting"
            );
            assert!(r.unwrap().features.is_empty());
        }
        // OfflineMode::Fail WITHOUT any cached configuration → error (never connected)
        {
            cfg.offline_mode = OfflineMode::Fail;
            cfg.configuration = Arc::new(Mutex::new(None));
            let r = cfg.get_configuration();
            assert!(
                r.is_err(),
                "Should error when no cached config was ever fetched"
            );
            assert_eq!(
                r.unwrap_err(),
                Error::Offline(CurrentModeOfflineReason::WebsocketClosed)
            );
        }

        {
            cfg.offline_mode = OfflineMode::Cache;
            {
                // No in-memory config yet → ConfigurationNotYetAvailable
                cfg.configuration = Arc::new(Mutex::new(None));
                let r = cfg.get_configuration();
                assert!(r.is_err());
                assert_eq!(r.unwrap_err(), Error::ConfigurationNotYetAvailable);
            }
            {
                // In-memory config exists → served via priority-1 stale-cache path
                cfg.configuration = Arc::new(Mutex::new(Some(Configuration::default())));
                let r = cfg.get_configuration();
                assert!(r.is_ok(), "Error: {}", r.unwrap_err());
                assert!(r.unwrap().features.is_empty());
            }
        }

        {
            // FallbackData with no in-memory config → falls back to FallbackData source
            let offline = AppConfigurationOffline::new(
                &example_configuration_enterprise_path,
                "dev",
                "blue-charge",
            )
            .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            cfg.configuration = Arc::new(Mutex::new(None));
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

        // OfflineMode::Fail WITH a stale in-memory config → serve it (thread may have
        // exited cleanly, e.g. after clean_up(), but config is still valid in RAM).
        {
            cfg.offline_mode = OfflineMode::Fail;
            cfg.configuration = Arc::new(Mutex::new(Some(Configuration::default())));
            let r = cfg.get_configuration();
            assert!(
                r.is_ok(),
                "OfflineMode::Fail must serve stale in-memory config even when thread is defunct"
            );
            assert!(r.unwrap().features.is_empty());
        }

        // OfflineMode::Fail WITHOUT any cached configuration → error
        {
            cfg.offline_mode = OfflineMode::Fail;
            cfg.configuration = Arc::new(Mutex::new(None));
            let r = cfg.get_configuration();
            assert!(r.is_err(), "Should error when no cached config available");
            assert_eq!(
                r.unwrap_err(),
                Error::ThreadInternalError("Thread finished with status: Ok(())".to_string())
            );
        }

        {
            cfg.offline_mode = OfflineMode::Cache;
            {
                // No in-memory config and thread is defunct → UnrecoverableError
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
                // In-memory config exists → served via priority-1 stale-cache path
                cfg.configuration = Arc::new(Mutex::new(Some(Configuration::default())));
                let r = cfg.get_configuration();
                assert!(r.is_ok(), "Error: {}", r.unwrap_err());
                assert!(r.unwrap().features.is_empty());
            }
        }

        {
            // FallbackData with no in-memory config → falls back to FallbackData source
            let offline = AppConfigurationOffline::new(
                &example_configuration_enterprise_path,
                "dev",
                "blue-charge",
            )
            .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            cfg.configuration = Arc::new(Mutex::new(None));
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert_eq!(r.unwrap().features.len(), 5);
        }
    }
}

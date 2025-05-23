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

use std::sync::{Arc, Mutex};

use super::current_mode::CurrentModeOfflineReason;
use super::update_thread_worker::UpdateThreadWorker;
use super::{CurrentMode, Error, OfflineMode, Result};
use crate::client::configuration::Configuration;
use crate::network::http_client::ServerClient;
use crate::utils::{ThreadHandle, ThreadStatus};
use crate::{ConfigurationId, ConfigurationProvider};

/// A [`ConfigurationProvider`] that keeps the configuration updated with some
/// third-party source using an asyncronous mechanism.
pub trait LiveConfiguration: ConfigurationProvider {
    /// Utility method to know the current status of the inner thread that keeps
    /// the configuration synced with the server.
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>>;

    /// Utility method to get the current operating mode of the object.
    fn get_current_mode(&self) -> Result<CurrentMode>;
}

#[derive(Debug)]
pub struct LiveConfigurationImpl {
    /// Configuration object that will be returned to consumers. This is also the object
    /// that the thread in the backend will be updating.
    configuration: Arc<Mutex<Option<Configuration>>>,

    /// Current operation mode.
    current_mode: Arc<Mutex<CurrentMode>>,

    /// Handler to the internal thread that takes care of updating the [`LiveConfigurationImpl::configuration`].
    update_thread: ThreadHandle<Result<()>>,

    /// Behaviour while the server is offline
    offline_mode: OfflineMode,
}

impl LiveConfigurationImpl {
    /// Creates a new [`LiveConfigurationImpl`] object and starts a thread running an instance
    /// of [`UpdateThreadWorker`].
    pub fn new<T: ServerClient>(
        offline_mode: OfflineMode,
        server_client: T,
        configuration_id: ConfigurationId,
    ) -> Self {
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let worker = UpdateThreadWorker::new(
            server_client,
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let update_thread =
            ThreadHandle::new(move |terminator_receiver| worker.run(terminator_receiver));

        Self {
            configuration,
            update_thread,
            current_mode,
            offline_mode,
        }
    }

    /// Returns the current [`Configuration`] after considering the [`CurrentMode`] and the [`OfflineMode`]
    /// configured for this object.
    fn get_configuration(&self) -> Result<Configuration> {
        // TODO: Can we return a reference instead?
        match &*self.current_mode.lock()? {
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
            },
        }
    }
}

impl ConfigurationProvider for LiveConfigurationImpl {
    fn get_feature_ids(&self) -> crate::Result<Vec<String>> {
        self.get_configuration()?.get_feature_ids()
    }

    fn get_feature(
        &self,
        feature_id: &str,
    ) -> crate::Result<crate::client::feature_snapshot::FeatureSnapshot> {
        self.get_configuration()?.get_feature(feature_id)
    }

    fn get_property_ids(&self) -> crate::Result<Vec<String>> {
        self.get_configuration()?.get_property_ids()
    }

    fn get_property(
        &self,
        property_id: &str,
    ) -> crate::Result<crate::client::property_snapshot::PropertySnapshot> {
        self.get_configuration()?.get_property(property_id)
    }
}

impl LiveConfiguration for LiveConfigurationImpl {
    fn get_thread_status(&mut self) -> ThreadStatus<Result<()>> {
        self.update_thread.get_thread_status()
    }

    fn get_current_mode(&self) -> Result<CurrentMode> {
        Ok(self.current_mode.lock()?.clone())
    }
}

#[cfg(test)]
mod tests {

    use std::sync::mpsc::{self, RecvError};

    use rstest::rstest;

    use crate::models::tests::{
        configuration_property1_enabled, example_configuration_enterprise_path,
    };

    use crate::network::http_client::WebsocketReader;
    use crate::network::live_configuration::update_thread_worker::SERVER_HEARTBEAT;
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
            rx: mpsc::Receiver<crate::models::ConfigurationJson>,
            websocket_rx: mpsc::Receiver<WebsocketReaderMock>,
        }
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(self.rx.recv().unwrap())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
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
        let configuration = crate::models::tests::configuration_feature1_enabled();
        let config = {
            // allow thread to start (unblock)
            get_configuration_tx.send(configuration).unwrap();
            websocket_factory_tx
                .send(WebsocketReaderMock {
                    rx: read_msg_rx,
                    tx: read_msg_ping_tx,
                })
                .unwrap();

            // Wait for thread to do some work and then to wait on websocket
            read_msg_ping_rx.recv().unwrap();
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
            current_mode: Arc::new(Mutex::new(CurrentMode::Online)),
            update_thread: ThreadHandle {
                _thread_termination_sender: tx,
                thread_handle: None,
                finished_thread_status_cached: None,
            },
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
            let offline =
                AppConfigurationOffline::new(&example_configuration_enterprise_path, "dev")
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
            current_mode: Arc::new(Mutex::new(CurrentMode::Offline(
                CurrentModeOfflineReason::ConfigurationDataInvalid,
            ))),
            update_thread: ThreadHandle {
                _thread_termination_sender: tx,
                thread_handle: None,
                finished_thread_status_cached: None,
            },
        };

        {
            cfg.offline_mode = OfflineMode::Fail;
            let r = cfg.get_configuration();
            assert!(r.is_err(), "Error: {}", r.unwrap_err());
            assert_eq!(
                r.unwrap_err(),
                Error::Offline(CurrentModeOfflineReason::ConfigurationDataInvalid)
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
            let offline =
                AppConfigurationOffline::new(&example_configuration_enterprise_path, "dev")
                    .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert_eq!(r.unwrap().features.len(), 6);
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
            current_mode: Arc::new(Mutex::new(CurrentMode::Defunct(Ok(())))),
            update_thread: ThreadHandle {
                _thread_termination_sender: tx,
                thread_handle: None,
                finished_thread_status_cached: None,
            },
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
            let offline =
                AppConfigurationOffline::new(&example_configuration_enterprise_path, "dev")
                    .unwrap();
            cfg.offline_mode = OfflineMode::FallbackData(offline);
            let r = cfg.get_configuration();
            assert!(r.is_ok(), "Error: {}", r.unwrap_err());
            assert_eq!(r.unwrap().features.len(), 6);
        }
    }
}

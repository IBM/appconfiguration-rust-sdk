// (C) Copyright IBM Corp. 2024.
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

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use super::current_mode::{CurrentMode, CurrentModeOfflineReason};
use super::{Error, Result};
use crate::client::configuration::Configuration;
use crate::network::http_client::{ServerClient, WebsocketReader};
use crate::network::NetworkError;
use crate::ConfigurationId;

pub(crate) const SERVER_HEARTBEAT: &str = "test message";

pub(crate) struct UpdateThreadWorker<T: ServerClient> {
    server_client: T,
    configuration_id: ConfigurationId,
    configuration: Arc<Mutex<Option<Configuration>>>,
    current_mode: Arc<Mutex<CurrentMode>>,
}

impl<T: ServerClient> UpdateThreadWorker<T> {
    pub(crate) fn new(
        server_client: T,
        configuration_id: ConfigurationId,
        configuration: Arc<Mutex<Option<Configuration>>>,
        current_mode: Arc<Mutex<CurrentMode>>,
    ) -> Self {
        Self {
            server_client,
            configuration_id,
            configuration,
            current_mode,
        }
    }
    fn run_internal(&self, thread_termination_receiver: Receiver<()>) -> Result<()> {
        'outer: loop {
            // When want to have a configuration available asap.
            // FIXME: Add test case for race condition: if there is a configuration change
            //        happening between this 'get_configuration_from_server' and the ws
            //        handshake we are missing those changes. The ws is not yet connected,
            //        so it won't receive the 'config_update' message and the Configuration
            //        we got in this call doesn't include those changes.
            self.update_configuration_from_server_and_current_mode()?;

            // Connect websocket
            let r = self
                .server_client
                .get_configuration_monitoring_websocket(&self.configuration_id);
            let mut socket = match r {
                Ok(socket) => socket,
                Err(e) => {
                    Self::recoverable_error(e)?;
                    continue 'outer;
                }
            };

            'inner: loop {
                // If the client is gone, we want to exit the loop so the socket is closed on our side, the thread will be terminanted
                match thread_termination_receiver.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    _ => {
                        break 'outer;
                    } // We are done
                }

                // Receive something from the websocket
                // BUG: If the WS doens't receive data, we are blocked here forever (until the parent process kills this thread).
                match self.handle_websocket_message(socket)? {
                    Some(ws) => socket = ws,
                    None => break 'inner, // Go and create another socket
                }
            }
        }
        Ok(())
    }

    /// Whenever this function returns, current_mode is a variant of Offline.
    pub(crate) fn run(&self, thread_termination_receiver: Receiver<()>) -> Result<()> {
        let result = self.run_internal(thread_termination_receiver);
        *self.current_mode.lock().unwrap() = CurrentMode::Defunct(result.clone());
        // TODO: maybe we do not need to return anything here anymore
        result
    }

    fn update_configuration_from_server_and_current_mode(&self) -> Result<()> {
        match self.server_client.get_configuration(&self.configuration_id) {
            Ok(config_json) => {
                match Configuration::new(&self.configuration_id.environment_id, config_json) {
                    Ok(config) => {
                        *self.configuration.lock()? = Some(config);
                        *self.current_mode.lock()? = CurrentMode::Online;
                    }
                    Err(_) => {
                        *self.current_mode.lock()? = CurrentMode::Offline(
                            CurrentModeOfflineReason::ConfigurationDataInvalid,
                        );
                    }
                };
                Ok(())
            }
            Err(e) => {
                Self::recoverable_error(e)?;
                let current_mode = self.current_mode.lock()?.clone();
                if let CurrentMode::Offline(_) = current_mode {
                } else {
                    *self.current_mode.lock()? =
                        CurrentMode::Offline(CurrentModeOfflineReason::FailedToGetNewConfiguration);
                }
                Ok(())
            }
        }
    }

    fn handle_websocket_message<WS: WebsocketReader>(&self, mut socket: WS) -> Result<Option<WS>> {
        match socket.read_msg() {
            Ok(msg) => match msg {
                tungstenite::Message::Text(utf8_bytes) => {
                    let current_mode_clone = self.current_mode.lock()?.clone();
                    match (utf8_bytes.as_str(), current_mode_clone) {
                        (SERVER_HEARTBEAT, CurrentMode::Offline(_)) => {
                            self.update_configuration_from_server_and_current_mode()?;
                        }
                        (SERVER_HEARTBEAT, CurrentMode::Online) => {}
                        _ => {
                            self.update_configuration_from_server_and_current_mode()?;
                        }
                    }
                    Ok(Some(socket))
                }
                tungstenite::Message::Close(_) => {
                    *self.current_mode.lock()? =
                        CurrentMode::Offline(CurrentModeOfflineReason::WebsocketClosed);
                    Ok(None)
                }
                _ => {
                    // Not specified in the WS protocol. We do nothing here.
                    Ok(Some(socket))
                }
            },
            Err(_) => {
                *self.current_mode.lock()? =
                    CurrentMode::Offline(CurrentModeOfflineReason::WebsocketError);
                Ok(None)
            }
        }
    }

    /// Whether the [`NetworkError`] will be permanent (it depends on static data) or we
    /// want to keep running the thread in case it eventually succeeds
    fn recoverable_error(error: NetworkError) -> Result<()> {
        match error {
            NetworkError::ReqwestError(_) => Ok(()),
            NetworkError::TungsteniteError(_) => Ok(()),
            NetworkError::ProtocolError => Ok(()),
            NetworkError::ContactToServerLost => Ok(()),
            NetworkError::UrlParseError(e) => Err(Error::UnrecoverableError(e)),
            NetworkError::InvalidHeaderValue(e) => Err(Error::UnrecoverableError(e)),
            NetworkError::CannotAcquireLock => Err(Error::CannotAcquireLock),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct WebsocketMockReader {
        message: Option<tungstenite::error::Result<tungstenite::Message>>,
    }
    impl WebsocketReader for WebsocketMockReader {
        fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
            self.message.take().unwrap()
        }
    }
    #[test]
    fn test_update_configuration_happy() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(crate::models::tests::configuration_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(*current_mode.lock().unwrap(), CurrentMode::Online);
    }

    #[test]
    fn test_update_configuration_invalid_configuration() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(crate::models::tests::configuration_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id =
            ConfigurationId::new("".into(), "non_existing_environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::ConfigurationDataInvalid)
        );
    }

    #[test]
    fn test_update_configuration_network_error_recoverable() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Err(crate::NetworkError::ProtocolError)
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Online));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        // check if we transition from online to offline:
        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::FailedToGetNewConfiguration)
        );

        // Test if offline mode is preserved:
        *current_mode.lock().unwrap() =
            CurrentMode::Offline(CurrentModeOfflineReason::Initializing);
        let r = worker.update_configuration_from_server_and_current_mode();
        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::Initializing)
        );
    }

    #[test]
    fn test_update_configuration_network_error_non_recoverable() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Err(crate::NetworkError::CannotAcquireLock)
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Online));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        // check if we transition from online to offline:
        assert!(r.is_err());
        // If error is returned, we do not guarantee anything on configuration and current_mode.
    }

    #[test]
    fn test_handle_websocket_msg_good() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(crate::models::tests::configuration_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        // we expect after heartbeat to change to online:
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(*current_mode.lock().unwrap(), CurrentMode::Online);

        // A repeated heartbeat should not re-fetch config (noop once online)
        *configuration.lock().unwrap() = None;
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(*current_mode.lock().unwrap(), CurrentMode::Online);

        // Any other message type is a noop
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::Ping(tungstenite::Bytes::new()))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(*current_mode.lock().unwrap(), CurrentMode::Online);

        // any other text message should lead to a config update (None -> Some)
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(""))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(*current_mode.lock().unwrap(), CurrentMode::Online);

        // After websocket is closed, it is consumed and we are offline
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::Close(None))),
        });
        assert!(r.unwrap().is_none()); // WS consumed
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::WebsocketClosed)
        );
    }

    #[test]
    fn test_handle_websocket_update_config_fail() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Err(crate::NetworkError::UrlParseError("".to_string()))
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        // A heartbeat in offline mode will trigger config retrieval.
        // Test that errors are propagated:
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.is_err());

        // Any other message will trigger config retrieval.
        // Test that errors are propagated:
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(""))),
        });
        assert!(r.is_err());

        // additionally we check that a heartbeat when online is a noop
        *current_mode.lock().unwrap() = CurrentMode::Online;
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.is_ok());
    }

    #[test]
    fn test_handle_websocket_read_failure() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                unreachable!()
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Online));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        // Errors on websocket lead to websocket being closed / consumed:
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Err(tungstenite::Error::AttackAttempt)),
        });
        assert!(r.unwrap().is_none());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::WebsocketError)
        );
    }

    #[test]
    fn test_run_initial_config_retrieval_fails_unrecoverably() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Err(crate::NetworkError::UrlParseError("".to_string()))
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                unreachable!() as crate::NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Online));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );
        let (_, rx) = std::sync::mpsc::channel();

        let r = worker.run(rx);
        assert!(r.is_err());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Defunct(Err(super::Error::UnrecoverableError("".into())))
        );
    }

    #[test]
    fn test_run_get_websocket_fail() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(crate::models::tests::configuration_feature1_enabled())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> crate::NetworkResult<impl WebsocketReader> {
                Err::<WebsocketMockReader, _>(crate::NetworkError::InvalidHeaderValue("".into()))
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Online));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );
        let (_, rx) = std::sync::mpsc::channel();

        let r = worker.run(rx);
        assert!(r.is_err());
        assert_eq!(
            *current_mode.lock().unwrap(),
            CurrentMode::Defunct(Err(super::Error::UnrecoverableError("".into())))
        );
    }
}

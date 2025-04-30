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

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use super::current_mode::CurrentModeOfflineReason;
use super::CurrentMode;
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

    /// Executes and _endless_ loop implementing the following behaviour:
    /// 1. Connects to the websocket
    /// 2. Retrieves some initial configuration
    /// 3. Listen to all messages coming from the websocket
    ///
    /// This loop will try to keep the connection open until any of these events happen:
    /// * it receives a termination signal via the `thread_termination_receiver` receiver.
    /// * it happens any unrecoverable error (see [`UpdateThreadWorker::recoverable_error`])
    fn run_internal(&self, thread_termination_receiver: Receiver<()>) -> Result<()> {
        'outer: loop {
            // Connect websocket, now we are receiving all the update notifications
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

            // Get the initial configuration
            self.update_configuration_from_server_and_current_mode()?;

            'inner: loop {
                // If the client is gone, we want to exit the loop so the socket is closed on our side, the thread will be terminanted
                match thread_termination_receiver.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    _ => return Ok(()),
                }

                // Receive something from the websocket
                // BUG: If the WS doens't receive data, we are blocked here forever (until the parent process kills this thread).
                match self.handle_websocket_message(socket)? {
                    Some(ws) => socket = ws,
                    None => break 'inner, // Go and create another socket
                }
            }
        }
    }

    /// Executes [`UpdateThreadWorker::run_internal`] and forwards its result. When this method returns,
    /// [`UpdateThreadWorker::current_mode`] is set to [`CurrentMode::Defunct`].
    pub(crate) fn run(&self, thread_termination_receiver: Receiver<()>) -> Result<()> {
        let result = self.run_internal(thread_termination_receiver);
        *self.current_mode.lock().unwrap() = CurrentMode::Defunct(result.clone());
        // TODO: maybe we do not need to return anything here anymore
        result
    }

    /// Retrieves a new configuration from the server (using [`UpdateThreadWorker<T>::server_client`]) and
    /// updates the values of [`UpdateThreadWorker::configuration`] and [`UpdateThreadWorker::current_mode`]
    /// accordingly.
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

    /// Reads a message from the input `WS` and executes the associated behaviour:
    ///  * Nothing if it was a heartbeat.
    ///  * Updates the configuration and current mode.
    ///  * Goes to offline mode if there is any error or the connection has been closed.
    ///
    /// The function consumes the input `socket` if the connection have been closed or
    /// there is any error receiving the messages. It's up to the caller to implement
    /// the recovery procedure for these scenarios.
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

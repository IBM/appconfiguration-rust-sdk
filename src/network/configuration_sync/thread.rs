use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use super::current_mode::{CurrentMode, CurrentModeOfflineReason};
use super::{Error, Result};
use crate::network::NetworkError;
use crate::{
    client::configuration::Configuration,
    network::http_client::{ServerClient, WebsocketReader},
    ConfigurationId,
};

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

    pub(crate) fn run(&mut self, thread_termination_receiver: Receiver<()>) -> Result<()> {
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
                let current_mode = &*self.current_mode.lock()?;
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

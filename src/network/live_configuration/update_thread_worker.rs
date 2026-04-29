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

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::current_mode::CurrentModeOfflineReason;
use super::CurrentMode;
use super::{Error, Result};
use crate::models::Configuration;
use crate::network::http_client::{ServerClient, WebsocketReader};
use crate::network::serialization::ConfigurationJson;
use crate::network::live_configuration::current_mode;
use crate::network::NetworkError;
use crate::utils::Waitable;
use crate::client::{RuntimeEvent, RuntimeEventKind, RuntimeEventListener, RuntimeMode, RuntimeStatus};
use crate::ConfigurationId;

pub(crate) const SERVER_HEARTBEAT: &str = "test message";
const WEBSOCKET_READ_TIMEOUT: Duration = Duration::from_secs(65);
const RETRY_INITIAL_DELAY: Duration = Duration::from_secs(15);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(60 * 60);
const RETRY_MULTIPLIER: u32 = 2;
const CONNECTIVITY_PROBE_TARGET: &str = "dns.google:53";
const CONNECTIVITY_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct UpdateThreadWorker<T: ServerClient> {
    server_client: T,
    configuration_id: ConfigurationId,
    configuration: Arc<Mutex<Option<Configuration>>>,
    current_mode: Waitable<CurrentMode>,
    persistent_cache_path: Option<PathBuf>,
    retry_pending: Arc<AtomicBool>,
    runtime_event_listeners: Arc<Mutex<Vec<RuntimeEventListener>>>,
}

impl<T: ServerClient> UpdateThreadWorker<T> {
    pub(crate) fn new(
        server_client: T,
        configuration_id: ConfigurationId,
        configuration: Arc<Mutex<Option<Configuration>>>,
        current_mode: Waitable<CurrentMode>,
        runtime_event_listeners: Arc<Mutex<Vec<RuntimeEventListener>>>,
    ) -> Self {
        Self {
            server_client,
            configuration_id,
            configuration,
            current_mode,
            persistent_cache_path: None,
            retry_pending: Arc::new(AtomicBool::new(false)),
            runtime_event_listeners,
        }
    }

    pub(crate) fn with_persistent_cache_file(mut self, path: impl AsRef<Path>) -> Self {
        self.persistent_cache_path = Some(path.as_ref().to_path_buf());
        self
    }

    fn has_internet_connectivity() -> bool {
        let addresses = match CONNECTIVITY_PROBE_TARGET.to_socket_addrs() {
            Ok(addresses) => addresses.collect::<Vec<SocketAddr>>(),
            Err(_) => return true,
        };

        addresses.into_iter().any(|address| {
            std::net::TcpStream::connect_timeout(&address, CONNECTIVITY_PROBE_TIMEOUT).is_ok()
        })
    }

    fn is_local_connection_refused(error: &NetworkError) -> bool {
        match error {
            NetworkError::ReqwestError(reqwest_error) => reqwest_error.is_connect(),
            NetworkError::TungsteniteError(tungstenite::Error::Io(io_error)) => {
                io_error.raw_os_error().map(|os_error| os_error == 61).unwrap_or(false)
            }
            NetworkError::ContactToServerLost => true,
            _ => false,
        }
    }

    fn classify_connectivity_error(&self, error: &NetworkError) -> CurrentModeOfflineReason {
        match error {
            NetworkError::ReqwestError(_)
            | NetworkError::ContactToServerLost
            | NetworkError::WebsocketTimeout
            | NetworkError::TokenProviderError(_)
            | NetworkError::TungsteniteError(_) => {
                if Self::is_local_connection_refused(error) || Self::has_internet_connectivity() {
                    CurrentModeOfflineReason::WebsocketError
                } else {
                    CurrentModeOfflineReason::InternetConnectivityError
                }
            }
            NetworkError::ProtocolError
            | NetworkError::ConfigurationDataError(_)
            | NetworkError::WebsocketHttpStatus { .. } => CurrentModeOfflineReason::WebsocketError,
            NetworkError::UrlParseError(_)
            | NetworkError::InvalidHeaderValue(_)
            | NetworkError::CannotAcquireLock => CurrentModeOfflineReason::WebsocketError,
        }
    }

    fn get_runtime_status(&self) -> Result<RuntimeStatus> {
        let mode = self.current_mode.get()?;
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
        Ok(status)
    }

    fn emit_runtime_event(&self, kind: RuntimeEventKind) -> Result<()> {
        let listeners = self.runtime_event_listeners.lock()?.clone();
        let event = RuntimeEvent {
            kind,
            status: self.get_runtime_status()?,
        };

        for listener in listeners {
            listener(event.clone());
        }

        Ok(())
    }

    fn emit_offline_runtime_event(&self, offline_reason: CurrentModeOfflineReason) -> Result<()> {
        let kind = match offline_reason {
            CurrentModeOfflineReason::WebsocketClosed => RuntimeEventKind::Closed,
            CurrentModeOfflineReason::WebsocketHeartbeatTimeout => RuntimeEventKind::HeartbeatTimeout,
            _ => RuntimeEventKind::Disconnected,
        };

        self.current_mode.set(CurrentMode::Offline(offline_reason))?;
        self.emit_runtime_event(kind)
    }

    fn emit_refresh_failure_event(&self) -> Result<()> {
        self.emit_runtime_event(RuntimeEventKind::RefreshFailure)
    }

    fn calculate_retry_delay(attempt: u32) -> Duration {
        let multiplier = RETRY_MULTIPLIER.saturating_pow(attempt);
        let base_delay = std::cmp::min(RETRY_INITIAL_DELAY.saturating_mul(multiplier), RETRY_MAX_DELAY);
        let base_millis = base_delay.as_millis() as u64;
        let jitter_range = ((base_millis as f64) * 0.3f64) as u64;
        let jitter_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos() as u64)
            .unwrap_or(0);
        let jitter_offset = if jitter_range == 0 {
            0
        } else {
            jitter_seed % (jitter_range.saturating_mul(2).saturating_add(1))
        };
        let delay_millis = base_millis
            .saturating_sub(jitter_range)
            .saturating_add(jitter_offset);
        Duration::from_millis(delay_millis)
    }

    fn wait_before_retry(
        &self,
        thread_termination_receiver: &Receiver<()>,
        attempt: u32,
    ) -> std::result::Result<(), ()> {
        if self.retry_pending.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let delay = Self::calculate_retry_delay(attempt);
        let result = match thread_termination_receiver.recv_timeout(delay) {
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(()),
        };
        self.retry_pending.store(false, Ordering::SeqCst);
        result
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
        let mut retry_attempt = 0;

        'outer: loop {
            // Connect websocket, now we are receiving all the update notifications
            let r = self
                .server_client
                .get_configuration_monitoring_websocket(&self.configuration_id);
            let mut socket = match r {
                Ok(socket) => {
                    retry_attempt = 0;
                    self.retry_pending.store(false, Ordering::SeqCst);
                    socket
                }
                Err(e) => {
                    let offline_reason = self.classify_connectivity_error(&e);
                    Self::recoverable_error(e)?;
                    self.emit_offline_runtime_event(offline_reason)?;
                    if self
                        .wait_before_retry(&thread_termination_receiver, retry_attempt)
                        .is_err()
                    {
                        return Ok(());
                    }
                    retry_attempt = retry_attempt.saturating_add(1);
                    continue 'outer;
                }
            };

            // Get the initial configuration. The first fetch failure should still transition
            // out of Initializing so callers do not remain stuck forever in that bootstrap
            // state while the worker continues retrying.
            let initial_fetch_succeeded =
                self.update_configuration_from_server_and_current_mode_with_reason(
                    CurrentModeOfflineReason::FailedToGetNewConfiguration,
                    true,
                )?;
            if initial_fetch_succeeded {
                retry_attempt = 0;
            } else {
                if self
                    .wait_before_retry(&thread_termination_receiver, retry_attempt)
                    .is_err()
                {
                    return Ok(());
                }
                retry_attempt = retry_attempt.saturating_add(1);
                continue 'outer;
            }
            let _ = socket.set_read_timeout(Some(WEBSOCKET_READ_TIMEOUT));

            'inner: loop {
                // If the client is gone, we want to exit the loop so the socket is closed on our side, the thread will be terminanted
                match thread_termination_receiver.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    _ => return Ok(()),
                }

                // Receive something from the websocket.
                // A read timeout is configured above, so silent/stale websocket connections are
                // converted into timeout handling instead of blocking forever.
                match self.handle_websocket_message(socket)? {
                    Some(ws) => socket = ws,
                    None => {
                        self.emit_offline_runtime_event(CurrentModeOfflineReason::WebsocketError)?;
                        if self
                            .wait_before_retry(&thread_termination_receiver, retry_attempt)
                            .is_err()
                        {
                            return Ok(());
                        }
                        retry_attempt = retry_attempt.saturating_add(1);
                        break 'inner; // Go and create another socket
                    }
                }
            }
        }
    }

    /// Executes [`UpdateThreadWorker::run_internal`] and forwards its result. When this method returns,
    /// [`UpdateThreadWorker::current_mode`] is set to [`CurrentMode::Defunct`].
    pub(crate) fn run(&self, thread_termination_receiver: Receiver<()>) -> Result<()> {
        let result = self.run_internal(thread_termination_receiver);
        let _ = self.current_mode.set(CurrentMode::Defunct(result.clone()));
        let _ = self.emit_runtime_event(RuntimeEventKind::Closed);
        result
    }

    /// Retrieves a new configuration from the server (using [`UpdateThreadWorker<T>::server_client`]) and
    /// updates the values of [`UpdateThreadWorker::configuration`] and [`UpdateThreadWorker::current_mode`]
    /// accordingly.
    ///
    /// Returns:
    /// - `Ok(true)` when a fresh configuration was fetched successfully
    /// - `Ok(false)` when the failure was recoverable and the worker should remain alive
    fn update_configuration_from_server_and_current_mode(&self) -> Result<bool> {
        self.update_configuration_from_server_and_current_mode_with_reason(
            CurrentModeOfflineReason::FailedToGetNewConfiguration,
            true,
        )
    }

    fn update_configuration_from_server_and_current_mode_with_reason(
        &self,
        default_offline_reason: CurrentModeOfflineReason,
        update_runtime_state_on_failure: bool,
    ) -> Result<bool> {
        match self.server_client.get_configuration(&self.configuration_id) {
            Ok(config) => {
                if let Some(path) = &self.persistent_cache_path {
                    let _ = path;
                }

                let mut current_config = self.configuration.lock()?;
                *current_config = Some(config);

                self.current_mode.set(CurrentMode::Online)?;
                self.emit_runtime_event(RuntimeEventKind::RefreshSuccess)?;

                Ok(true)
            }
            Err(e) => {
                let classified_reason = self.classify_connectivity_error(&e);
                let offline_reason = match default_offline_reason {
                    CurrentModeOfflineReason::FailedToGetNewConfiguration => {
                        CurrentModeOfflineReason::FailedToGetNewConfiguration
                    }
                    _ => match classified_reason {
                        CurrentModeOfflineReason::InternetConnectivityError => {
                            CurrentModeOfflineReason::InternetConnectivityError
                        }
                        _ => default_offline_reason,
                    },
                };

                Self::recoverable_error(e)?;
                if update_runtime_state_on_failure {
                    self.emit_offline_runtime_event(offline_reason)?;
                }
                self.emit_refresh_failure_event()?;

                Ok(false)
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
                    let current_mode_clone = self.current_mode.get()?;
                    match (utf8_bytes.as_str(), current_mode_clone) {
                        (SERVER_HEARTBEAT, CurrentMode::Offline(_)) => {
                            self.update_configuration_from_server_and_current_mode_with_reason(
                                CurrentModeOfflineReason::FailedToGetNewConfiguration,
                                true,
                            )?;
                        }
                        (SERVER_HEARTBEAT, CurrentMode::Online) => {}
                        _ => {
                            self.update_configuration_from_server_and_current_mode_with_reason(
                                CurrentModeOfflineReason::FailedToGetNewConfiguration,
                                true,
                            )?;
                        }
                    }
                    Ok(Some(socket))
                }
                tungstenite::Message::Close(_) => {
                    self.emit_offline_runtime_event(CurrentModeOfflineReason::WebsocketClosed)?;
                    Ok(None)
                }
                _ => {
                    // Not specified in the WS protocol. We do nothing here.
                    Ok(Some(socket))
                }
            },
            Err(error) => {
                let offline_reason = if matches!(
                    error,
                    tungstenite::Error::Io(ref io_error)
                        if io_error.kind() == std::io::ErrorKind::WouldBlock
                            || io_error.kind() == std::io::ErrorKind::TimedOut
                ) {
                    CurrentModeOfflineReason::WebsocketHeartbeatTimeout
                } else {
                    CurrentModeOfflineReason::WebsocketError
                };

                self.emit_offline_runtime_event(offline_reason)?;
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
            NetworkError::WebsocketTimeout => Ok(()),
            NetworkError::TokenProviderError(_) => Ok(()),
            NetworkError::WebsocketHttpStatus { status_code, message } => {
                if (400..500).contains(&status_code) && status_code != 429 {
                    Err(Error::UnrecoverableError(message))
                } else {
                    Ok(())
                }
            }
            NetworkError::UrlParseError(e) => Err(Error::UnrecoverableError(e)),
            NetworkError::InvalidHeaderValue(e) => Err(Error::UnrecoverableError(e)),
            NetworkError::CannotAcquireLock => Err(Error::CannotAcquireLock),
            NetworkError::ConfigurationDataError(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{network::NetworkResult, ConfigurationDataError};

    use super::*;

    struct WebsocketMockReader {
        message: Option<tungstenite::error::Result<tungstenite::Message>>,
    }
    impl WebsocketReader for WebsocketMockReader {
        fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
            self.message.take().unwrap()
        }

        fn set_read_timeout(&mut self, _timeout: Option<Duration>) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn test_update_configuration_happy() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Ok(crate::network::serialization::fixtures::configuration_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode =
            Waitable::new(CurrentMode::Offline(CurrentModeOfflineReason::Initializing));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);
    }

    #[test]
    fn test_update_configuration_invalid_configuration() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Err(ConfigurationDataError::EnvironmentNotFound(
                    "environment not in response".to_string(),
                )
                .into())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "not used".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode =
            Waitable::new(CurrentMode::Offline(CurrentModeOfflineReason::Initializing));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::FailedToGetNewConfiguration)
        );
    }

    #[test]
    fn test_update_configuration_protocol_error_recoverable() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Err(NetworkError::ProtocolError)
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::FailedToGetNewConfiguration)
        );
    }

    #[test]
    fn test_update_configuration_reqwest_error_classified_as_connectivity_issue() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Err(NetworkError::ContactToServerLost)
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        let r = worker.update_configuration_from_server_and_current_mode_with_reason(
            CurrentModeOfflineReason::InternetConnectivityError,
            true,
        );

        assert!(r.is_ok());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::InternetConnectivityError)
        );
    }

    #[test]
    fn test_update_configuration_network_error_non_recoverable() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Err(NetworkError::CannotAcquireLock)
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        let r = worker.update_configuration_from_server_and_current_mode();

        // check if we transition from online to offline:
        assert!(r.is_err());
        // If error is returned, we do not guarantee anything on configuration and current_mode.
    }

    #[test]
    fn test_handle_websocket_when_get_configuration_succeeds() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Ok(crate::network::serialization::fixtures::configuration_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode =
            Waitable::new(CurrentMode::Offline(CurrentModeOfflineReason::Initializing));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        // we expect after heartbeat to change to online:
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });

        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // A repeated heartbeat should not re-fetch config (noop once online)
        *configuration.lock().unwrap() = None;
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // Any other message type is a noop
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::Ping(tungstenite::Bytes::new()))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // any other text message should lead to a config update (None -> Some)
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(""))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // After websocket is closed, it is consumed and we are offline
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::Close(None))),
        });
        assert!(r.unwrap().is_none()); // WS consumed
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::WebsocketClosed)
        );
    }

    #[test]
    fn test_handle_websocket_update_when_get_configuration_fails() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Err(NetworkError::UrlParseError("".to_string()))
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode =
            Waitable::new(CurrentMode::Offline(CurrentModeOfflineReason::Initializing));

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
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

        // Additionally we check that a heartbeat when online is a noop
        current_mode.set(CurrentMode::Online).unwrap();
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
            ) -> NetworkResult<Configuration> {
                unreachable!()
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );

        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Err(tungstenite::Error::AttackAttempt)),
        });

        // Websocket read errors are recoverable -> Ok(_) is returned
        assert!(r.is_ok());

        // websocket read error causes websocket to not be given back (consumed)
        assert!(r.unwrap().is_none());

        // websocket read error changes current_mode to Offline
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::WebsocketError)
        );
    }

    #[test]
    fn test_run_initial_config_retrieval_fails_unrecoverably() {
        struct ServerClientMock {
            tx: std::sync::mpsc::Sender<String>,
        }
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                self.tx.send("get_configuration".to_string()).unwrap();
                Err(NetworkError::UrlParseError("".to_string()))
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                self.tx
                    .send("get_configuration_monitoring_websocket".to_string())
                    .unwrap();
                Ok(WebsocketMockReader { message: None })
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let (tx_serverclient_call_logs, rx_serverclient_call_logs) = std::sync::mpsc::channel();
        let worker = UpdateThreadWorker::new(
            ServerClientMock {
                tx: tx_serverclient_call_logs,
            },
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (_, rx_thread_terminator) = std::sync::mpsc::channel();

        let r = worker.run(rx_thread_terminator);
        assert!(r.is_err());
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Defunct(Err(Error::UnrecoverableError("".into())))
        );

        // We first called the websocket creation, and then get the configuration. This way we
        // are not loosing configuration updates. Every update notification will be waiting in
        // the websocket while we work with the initial configuration.
        assert_eq!(
            rx_serverclient_call_logs.recv().unwrap(),
            "get_configuration_monitoring_websocket".to_string()
        );
        assert_eq!(
            rx_serverclient_call_logs.recv().unwrap(),
            "get_configuration".to_string()
        );
        assert_eq!(
            rx_serverclient_call_logs.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
    }

    #[test]
    fn test_run_get_websocket_fail() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Ok(crate::network::serialization::fixtures::configuration_feature1_enabled())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                Err::<WebsocketMockReader, _>(NetworkError::InvalidHeaderValue("".into()))
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (_, rx) = std::sync::mpsc::channel();

        let r = worker.run(rx);
        assert!(r.is_err());
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Defunct(Err(Error::UnrecoverableError("".into())))
        );
    }

    #[test]
    fn test_run_thread_terminated() {
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Ok(crate::network::serialization::fixtures::configuration_feature1_enabled())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                Ok(WebsocketMockReader {
                    message: Some(Err(tungstenite::Error::AttackAttempt)),
                })
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let worker = UpdateThreadWorker::new(
            ServerClientMock {},
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        drop(tx);
        let r = worker.run(rx);
        assert!(r.is_ok());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Defunct(Ok(())));
    }

    #[test]
    fn test_run_websocket_reconnect() {
        struct ServerClientMock {
            rx: std::sync::mpsc::Receiver<NetworkResult<WebsocketMockReader>>,
        }
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<Configuration> {
                Ok(crate::network::serialization::fixtures::configuration_feature1_enabled())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                self.rx.recv().unwrap()
            }
        }
        let configuration_id = ConfigurationId::new("".into(), "environment_id".into(), "".into());
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Waitable::new(CurrentMode::Online);

        let (get_ws_tx, get_ws_rx) = std::sync::mpsc::channel();

        let server_client = ServerClientMock { rx: get_ws_rx };

        get_ws_tx
            .send(Ok(WebsocketMockReader {
                message: Some(Err(tungstenite::Error::AttackAttempt)),
            }))
            .unwrap();
        get_ws_tx
            .send(Err(NetworkError::CannotAcquireLock))
            .unwrap();

        let worker = UpdateThreadWorker::new(
            server_client,
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (_terminate_tx, terminate_rx) = std::sync::mpsc::channel();
        let r = worker.run(terminate_rx);

        // We assert that the websocket was attempted to be created 2 times:
        // Fist time successfully, but with a websocket returning errors on read causing reconnect
        // Second time (reconnect attempt) fails with CannotAcquireLock error.
        // The second fails WS creation is unrecoverable, which we can test:
        assert_eq!(r.unwrap_err(), Error::CannotAcquireLock);
    }
}

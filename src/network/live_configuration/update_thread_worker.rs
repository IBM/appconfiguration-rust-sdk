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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use super::CurrentMode;
use super::current_mode::CurrentModeOfflineReason;
use super::{Error, Result};
use crate::ConfigurationId;
use crate::client::{
    RuntimeEvent, RuntimeEventKind, RuntimeEventListener, RuntimeMode, RuntimeStatus,
};
use crate::models::Configuration;
use crate::network::NetworkError;
use crate::network::connectivity::check_internet_once;
use crate::network::http_client::{ServerClient, WebsocketReader};
use crate::utils::Waitable;
use rand::Rng;
use std::time::{Duration, Instant};
pub(crate) const SERVER_HEARTBEAT: &str = "test message";

const RETRY_INITIAL_DELAY: Duration = Duration::from_secs(15);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(60 * 60);
const RETRY_MULTIPLIER: u32 = 2;

const CONFIG_REFRESH_BASE_DELAY: Duration = Duration::from_secs(2 * 60); // 2 minutes
const CONFIG_REFRESH_CAP_DELAY: Duration = Duration::from_secs(60 * 60); // 1 hour
const CONFIG_REFRESH_MULTIPLIER: u32 = 2;

pub(crate) struct UpdateThreadWorker<T: ServerClient> {
    server_client: T,
    configuration_id: ConfigurationId,
    configuration: Arc<Mutex<Option<Configuration>>>,
    current_mode: Waitable<CurrentMode>,
    persistent_cache_path: Option<PathBuf>,
    retry_pending: Arc<AtomicBool>,
    runtime_event_listeners: Arc<Mutex<Vec<RuntimeEventListener>>>,
    is_connected: Arc<AtomicBool>,
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
            is_connected: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn with_persistent_cache_file(mut self, path: impl AsRef<Path>) -> Self {
        self.persistent_cache_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Single-shot (no retries) connectivity check used inside the backoff polling loop.
    /// Cheaper than the full 3-retry check (max 5s vs 15s) so the loop genuinely
    /// wakes ~every 500ms rather than ~every 5.5s while offline.
    fn has_internet_connectivity_once() -> bool {
        check_internet_once()
    }

    fn classify_connectivity_error(error: &NetworkError) -> CurrentModeOfflineReason {
        // We classify purely from the error type — NO blocking DNS check here.
        // Internet-state is managed reactively via the is_connected flag and the
        // polling loop in wait_before_retry.
        match error {
            NetworkError::TungsteniteError(tungstenite::Error::Io(io_error))
                if io_error.raw_os_error().map(|c| c == 61).unwrap_or(false) =>
            {
                // ECONNREFUSED — server is up but refusing: WS-level error, not internet loss
                CurrentModeOfflineReason::WebsocketError
            }
            NetworkError::ContactToServerLost => CurrentModeOfflineReason::WebsocketError,
            NetworkError::ReqwestError(_)
            | NetworkError::WebsocketTimeout
            | NetworkError::TokenProviderError(_)
            | NetworkError::TungsteniteError(_) => {
                // Could be internet or server — mark as generic WebsocketError.
                // The polling loop will detect true internet loss separately.
                CurrentModeOfflineReason::WebsocketError
            }
            NetworkError::ProtocolError
            | NetworkError::ConfigurationDataError(_)
            | NetworkError::WebsocketHttpStatus { .. }
            | NetworkError::DeserializationError(_)
            | NetworkError::UrlParseError(_)
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
            CurrentModeOfflineReason::WebsocketHeartbeatTimeout => {
                RuntimeEventKind::HeartbeatTimeout
            }
            _ => RuntimeEventKind::Disconnected,
        };

        self.current_mode
            .set(CurrentMode::Offline(offline_reason))?;
        self.emit_runtime_event(kind)
    }

    fn emit_refresh_failure_event(&self) -> Result<()> {
        self.emit_runtime_event(RuntimeEventKind::RefreshFailure)
    }

    fn calculate_retry_delay(attempt: u32) -> Duration {
        let multiplier = RETRY_MULTIPLIER.saturating_pow(attempt);
        let base_delay = std::cmp::min(
            RETRY_INITIAL_DELAY.saturating_mul(multiplier),
            RETRY_MAX_DELAY,
        );
        let base_millis = base_delay.as_millis() as u64;
        let jitter_range = ((base_millis as f64) * 0.3f64) as u64;
        let jitter_offset = if jitter_range == 0 {
            0
        } else {
            rand::rng().random_range(0..=jitter_range.saturating_mul(2))
        };
        let delay_millis = base_millis
            .saturating_sub(jitter_range)
            .saturating_add(jitter_offset);
        Duration::from_millis(delay_millis)
    }

    fn compute_base_delay_ms() -> u64 {
        let base_ms = CONFIG_REFRESH_BASE_DELAY.as_millis() as u64;
        let max_jitter_ms = 5000u64;
        let jitter_ms = rand::rng().random_range(0..=max_jitter_ms);
        base_ms + jitter_ms // 120,000–125,000ms (2.0–2.083 minutes)
    }

    /// Computes cap delay for config refresh with jitter (60:00–60:59 minutes).
    fn compute_cap_delay_ms() -> u64 {
        let base_ms = CONFIG_REFRESH_CAP_DELAY.as_millis() as u64; // 1 hour = 3,600,000ms
        let jitter_seconds = rand::rng().random_range(0u64..60);
        base_ms + (jitter_seconds * 1000) // 3,600,000–3,659,000ms (60:00–60:59 minutes)
    }

    /// Computes next config refresh delay with exponential backoff, capped.
    fn compute_next_config_refresh_delay(attempt: u32, cap_ms: u64) -> Duration {
        let base_ms = Self::compute_base_delay_ms();
        let exp_ms =
            base_ms.saturating_mul(CONFIG_REFRESH_MULTIPLIER.saturating_pow(attempt) as u64);
        let delay_ms = std::cmp::min(exp_ms, cap_ms);
        Duration::from_millis(delay_ms)
    }

    /// Calculates config refresh retry delay with exponential backoff and jitter.
    fn calculate_config_refresh_retry_delay(attempt: u32) -> Duration {
        let cap_ms = Self::compute_cap_delay_ms();
        Self::compute_next_config_refresh_delay(attempt, cap_ms)
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
        log::info!(
            "Websocket retry scheduled in {:.2} seconds (attempt #{})",
            delay.as_secs_f64(),
            attempt + 1
        );

        // Poll in short intervals so we can:
        //   (a) honour thread termination quickly, and
        //   (b) short-circuit the backoff the moment internet is restored.
        //
        // We use `has_internet_connectivity_once()` (single DNS attempt, 5s timeout) rather
        // than the full `has_internet_connectivity()` (3 attempts × 5s = up to 15s) so the
        // loop genuinely wakes up approximately every second rather than every 15s.
        const POLL_INTERVAL: Duration = Duration::from_millis(500);
        let deadline = Instant::now() + delay;
        let mut result = Ok(());

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let sleep_for = remaining.min(POLL_INTERVAL);

            match thread_termination_receiver.recv_timeout(sleep_for) {
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    result = Err(());
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }

            // Short-circuit if internet came back while we were waiting.
            // Only check when we know we are disconnected to avoid unnecessary DNS traffic.
            if !self.is_connected.load(Ordering::SeqCst) && Self::has_internet_connectivity_once() {
                log::info!(
                    "[CONNECTIVITY] Internet restored during backoff — reconnecting immediately"
                );
                self.is_connected.store(true, Ordering::SeqCst);
                break;
            }
        }

        self.retry_pending.store(false, Ordering::SeqCst);
        result
    }

    /// Waits before retrying config refresh with exponential backoff and jitter.
    fn wait_before_config_refresh_retry(
        &self,
        thread_termination_receiver: &Receiver<()>,
        attempt: u32,
    ) -> std::result::Result<(), ()> {
        if self.retry_pending.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let delay = Self::calculate_config_refresh_retry_delay(attempt);
        log::warn!(
            "Config refresh retry scheduled in {:.2} minutes (attempt #{})",
            delay.as_secs_f64() / 60.0,
            attempt + 1
        );

        let result = match thread_termination_receiver.recv_timeout(delay) {
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(()),
        };
        self.retry_pending.store(false, Ordering::SeqCst);
        result
    }

    /// Executes an endless loop that:
    /// 1. Connects to the WebSocket
    /// 2. Fetches the initial configuration via HTTP
    /// 3. Listens for live-update messages until the socket dies
    ///
    /// On any socket error the loop waits with exponential backoff and retries.
    /// The backoff is short-circuited as soon as internet connectivity is detected,
    /// so reconnection happens within ~1 second of internet restoration.
    ///
    /// The loop exits when:
    /// * a termination signal is received via `thread_termination_receiver`, OR
    /// * an unrecoverable error occurs (see [`UpdateThreadWorker::recoverable_error`])

    fn run_internal(&self, thread_termination_receiver: Receiver<()>) -> Result<()> {
        let mut websocket_retry_attempt = 0u32;
        let mut config_refresh_retry_attempt = 0u32;

        'outer: loop {
            log::debug!(
                "[WORKER] Connecting to WebSocket (attempt #{})",
                websocket_retry_attempt
            );

            // ── Step 1: Connect WebSocket ─────────────────────────────────────────────
            let r = self
                .server_client
                .get_configuration_monitoring_websocket(&self.configuration_id);

            let mut socket = match r {
                Ok(socket) => {
                    log::info!("[WORKER] WebSocket connected");
                    websocket_retry_attempt = 0;
                    config_refresh_retry_attempt = 0;
                    self.retry_pending.store(false, Ordering::SeqCst);
                    self.is_connected.store(true, Ordering::SeqCst);
                    self.emit_runtime_event(RuntimeEventKind::Connected)?;
                    socket
                }
                Err(e) => {
                    log::warn!(
                        "[WORKER] WebSocket connect failed (attempt #{}): {}",
                        websocket_retry_attempt,
                        e
                    );
                    let offline_reason = Self::classify_connectivity_error(&e);
                    // Mark disconnected so the poll loop detects restoration.
                    self.is_connected.store(false, Ordering::SeqCst);
                    Self::recoverable_error(e)?;
                    self.emit_offline_runtime_event(offline_reason)?;

                    if self
                        .wait_before_retry(&thread_termination_receiver, websocket_retry_attempt)
                        .is_err()
                    {
                        return Ok(());
                    }
                    // Reset counter if internet came back during the wait.
                    if self.is_connected.load(Ordering::SeqCst) {
                        log::info!(
                            "[WORKER] Internet restored during connect backoff — resetting counter"
                        );
                        websocket_retry_attempt = 0;
                    } else {
                        websocket_retry_attempt = websocket_retry_attempt.saturating_add(1);
                    }
                    continue 'outer;
                }
            };

            // ── Step 2: Fetch initial configuration via HTTP ──────────────────────────
            let initial_fetch_succeeded = self
                .update_configuration_from_server_and_current_mode_with_reason(
                    CurrentModeOfflineReason::FailedToGetNewConfiguration,
                    true,
                )?;

            if initial_fetch_succeeded {
                config_refresh_retry_attempt = 0;
            } else {
                log::warn!(
                    "[WORKER] Config fetch failed — backing off (attempt #{})",
                    config_refresh_retry_attempt
                );
                self.is_connected.store(false, Ordering::SeqCst);
                if self
                    .wait_before_config_refresh_retry(
                        &thread_termination_receiver,
                        config_refresh_retry_attempt,
                    )
                    .is_err()
                {
                    return Ok(());
                }
                config_refresh_retry_attempt = config_refresh_retry_attempt.saturating_add(1);
                continue 'outer;
            }

            // ── Step 3: Read messages until socket dies ───────────────────────────────
            //
            // When the socket dies (handle_websocket_message → None):
            //   • mark is_connected = false  (so the poll loop can detect restoration)
            //   • wait with backoff — the poll loop short-circuits as soon as internet
            //     returns, so reconnection happens within ~500ms of detection
            //   • then break back to the outer loop to re-establish the WS
            'inner: loop {
                match thread_termination_receiver.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    _ => return Ok(()),
                }

                match self.handle_websocket_message(socket)? {
                    Some(ws) => {
                        socket = ws;
                    }
                    None => {
                        log::warn!(
                            "[WORKER] Socket dead — backing off (attempt #{})",
                            websocket_retry_attempt
                        );
                        // Mark disconnected BEFORE the wait so the poll loop inside
                        // wait_before_retry can detect internet restoration immediately.
                        self.is_connected.store(false, Ordering::SeqCst);

                        if self
                            .wait_before_retry(
                                &thread_termination_receiver,
                                websocket_retry_attempt,
                            )
                            .is_err()
                        {
                            return Ok(());
                        }

                        if self.is_connected.load(Ordering::SeqCst) {
                            log::info!("[WORKER] Internet restored — reconnecting immediately");
                            websocket_retry_attempt = 0;
                        } else {
                            websocket_retry_attempt = websocket_retry_attempt.saturating_add(1);
                        }
                        break 'inner;
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
    ///
    fn update_configuration_from_server_and_current_mode_with_reason(
        &self,
        default_offline_reason: CurrentModeOfflineReason,
        update_runtime_state_on_failure: bool,
    ) -> Result<bool> {
        // Fetch configuration JSON from server
        match self
            .server_client
            .get_configuration_json(&self.configuration_id)
        {
            Ok(config_json) => {
                // Write to persistent cache if path is configured
                if let Some(path) = &self.persistent_cache_path {
                    if let Err(e) = config_json.write_to_file(path) {
                        log::warn!(
                            "Failed to write configuration to persistent cache at '{}': {}",
                            path.display(),
                            e
                        );
                    } else {
                        log::debug!(
                            "Successfully wrote configuration to persistent cache at '{}'",
                            path.display()
                        );
                    }
                }

                // Convert JSON to Configuration object
                let config = Configuration::new(
                    &self.configuration_id.environment_id,
                    &self.configuration_id.collection_id,
                    config_json,
                )
                .map_err(|e| {
                    Error::ThreadInternalError(format!("Failed to parse configuration: {}", e))
                })?;
                {
                    let mut current_config = self.configuration.lock()?;
                    *current_config = Some(config);
                }

                self.current_mode.set(CurrentMode::Online)?;
                self.emit_runtime_event(RuntimeEventKind::RefreshSuccess)?;

                Ok(true)
            }
            Err(e) => {
                let classified_reason = Self::classify_connectivity_error(&e);
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
                    if utf8_bytes.as_str() == SERVER_HEARTBEAT {
                        log::debug!(
                            "[WORKER] Heartbeat received — connection alive, no config fetch needed."
                        );
                        return Ok(Some(socket));
                    }

                    log::debug!("[WORKER] Config-change notification received — re-fetching.");
                    let jitter_ms = rand::rng().random_range(0..5000u64);
                    if jitter_ms > 0 {
                        log::debug!(
                            "[WORKER] Config refresh will start in {:.2}s (jitter).",
                            jitter_ms as f64 / 1000.0
                        );
                        std::thread::sleep(Duration::from_millis(jitter_ms));
                    }
                    self.update_configuration_from_server_and_current_mode_with_reason(
                        CurrentModeOfflineReason::FailedToGetNewConfiguration,
                        true,
                    )?;
                    Ok(Some(socket))
                }
                tungstenite::Message::Close(_) => {
                    self.emit_offline_runtime_event(CurrentModeOfflineReason::WebsocketClosed)?;

                    // CRITICAL FIX: The documentation states we must drive the close
                    // handshake. Flush the automatically queued close frame response out!
                    let _ = socket.flush_socket();

                    Ok(None) // Safe to drop now
                }

                tungstenite::Message::Ping(_bytes) => {
                    log::debug!("Received ping message from server");

                    // CRITICAL FIX: Tungstenite automatically queued a Pong response,
                    // but it won't send until we flush it! Without this, the server times you out.
                    if let Err(e) = socket.flush_socket() {
                        log::debug!("Failed to flush auto-pong response: {:?}", e);
                        return Ok(None); // Connection is dead
                    }

                    Ok(Some(socket))
                }
                _ => {
                    // Not specified in the WS protocol. We do nothing here.\
                    log::debug!("Received unexpected message: {:?}", msg);
                    Ok(Some(socket))
                }
            },
            Err(tungstenite::Error::Io(ref err))
                if err.kind() == std::io::ErrorKind::WouldBlock =>
            {
                // This triggers when the TCP read timeout fires (set via set_read_timeout).
                // The server sends a heartbeat at ~60s intervals; if we haven't received
                // anything within WEBSOCKET_READ_TIMEOUT_SECS (65s) it means the heartbeat
                // was missed — classify as a heartbeat timeout, not a clean close.
                log::debug!(
                    "Socket read timed out after {}s — no server heartbeat received.",
                    crate::network::http_client::WEBSOCKET_READ_TIMEOUT_SECS
                );
                self.emit_offline_runtime_event(
                    CurrentModeOfflineReason::WebsocketHeartbeatTimeout,
                )?;
                Ok(None)
            }
            Err(error) => {
                // This triggers on hard drops (ConnectionReset, BrokenPipe) and other websocket errors
                log::debug!("Websocket error detected, closing connection: {:?}", error);
                self.emit_offline_runtime_event(CurrentModeOfflineReason::WebsocketError)?;
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
            NetworkError::ConfigurationDataError(_) => Ok(()),
            NetworkError::WebsocketTimeout => Ok(()),
            NetworkError::TokenProviderError(_) => Ok(()),
            NetworkError::WebsocketHttpStatus {
                status_code,
                message,
            } => {
                if (400..500).contains(&status_code) && status_code != 429 {
                    Err(Error::UnrecoverableError(message))
                } else {
                    Ok(())
                }
            }
            NetworkError::DeserializationError(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ConfigurationDataError, network::NetworkResult};

    use super::*;

    struct WebsocketMockReader {
        message: Option<tungstenite::error::Result<tungstenite::Message>>,
    }
    impl WebsocketReader for WebsocketMockReader {
        fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
            self.message.take().unwrap()
        }
        fn flush_socket(&mut self) -> tungstenite::error::Result<()> {
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
            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
                Ok(crate::network::serialization::fixtures::configuration_json_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id =
            ConfigurationId::new("".into(), "environment_id".into(), "collection_id".into());
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

        let r = worker.update_configuration_from_server_and_current_mode_with_reason(
            CurrentModeOfflineReason::FailedToGetNewConfiguration,
            true,
        );

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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
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

        let r = worker.update_configuration_from_server_and_current_mode_with_reason(
            CurrentModeOfflineReason::FailedToGetNewConfiguration,
            true,
        );

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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
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

        let r = worker.update_configuration_from_server_and_current_mode_with_reason(
            CurrentModeOfflineReason::FailedToGetNewConfiguration,
            true,
        );

        // check if we transition from online to offline:
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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
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

        let r = worker.update_configuration_from_server_and_current_mode_with_reason(
            CurrentModeOfflineReason::FailedToGetNewConfiguration,
            true,
        );

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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
                Ok(crate::network::serialization::fixtures::configuration_json_feature1_enabled())
            }

            #[allow(unreachable_code)]
            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &ConfigurationId,
            ) -> NetworkResult<impl WebsocketReader> {
                unreachable!() as NetworkResult<WebsocketMockReader>
            }
        }
        let configuration_id =
            ConfigurationId::new("".into(), "environment_id".into(), "collection_id".into());
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

        // Heartbeat while offline → does NOT trigger a fetch (keep-alive only).
        // Configuration stays None, mode stays Offline.
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none()); // no fetch happened
        assert_eq!(
            current_mode.get().unwrap(),
            CurrentMode::Offline(CurrentModeOfflineReason::Initializing)
        );

        // A non-heartbeat text message IS a config-change notification → fetch triggered.
        // After a successful fetch the mode transitions to Online.
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(
                "collection_id:c1;environment_id:e1",
            ))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_some());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // Heartbeat while already online → still no fetch (keep-alive only).
        *configuration.lock().unwrap() = None;
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // Ping frames are a noop — no fetch, no state change
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::Ping(tungstenite::Bytes::new()))),
        });
        assert!(r.unwrap().is_some());
        assert!(configuration.lock().unwrap().is_none());
        assert_eq!(current_mode.get().unwrap(), CurrentMode::Online);

        // Another non-heartbeat text → config re-fetch
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(
                "collection_id:c1;environment_id:e1",
            ))),
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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
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

        // A heartbeat (= "test message") is a keep-alive only — no fetch, no error,
        // regardless of current mode.
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(SERVER_HEARTBEAT))),
        });
        assert!(r.is_ok()); // heartbeat is always a noop

        // A non-heartbeat text message triggers a config fetch.
        // The mock returns UrlParseError (unrecoverable) → propagated as Err.
        let r = worker.handle_websocket_message(WebsocketMockReader {
            message: Some(Ok(tungstenite::Message::text(
                "collection_id:c1;environment_id:e1",
            ))),
        });
        assert!(r.is_err());

        // Same behaviour when already Online: heartbeat = noop.
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
                Err(NetworkError::UrlParseError("".to_string()))
            }

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
                self.tx.send("get_configuration_json".to_string()).unwrap();
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

        // We first called the websocket creation, and then get the configuration JSON. This way
        // we are not losing configuration updates. Every update notification will be waiting in
        // the websocket while we work with the initial configuration.
        assert_eq!(
            rx_serverclient_call_logs.recv().unwrap(),
            "get_configuration_monitoring_websocket".to_string()
        );
        assert_eq!(
            rx_serverclient_call_logs.recv().unwrap(),
            "get_configuration_json".to_string()
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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
                Ok(crate::network::serialization::fixtures::configuration_json_feature1_enabled())
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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
                Ok(crate::network::serialization::fixtures::configuration_json_feature1_enabled())
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

            fn get_configuration_json(
                &self,
                _configuration_id: &ConfigurationId,
            ) -> NetworkResult<crate::network::serialization::ConfigurationJson> {
                Ok(crate::network::serialization::fixtures::configuration_json_feature1_enabled())
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

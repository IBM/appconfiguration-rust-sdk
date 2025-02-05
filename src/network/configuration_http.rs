use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use crate::{
    client::configuration::Configuration, ConfigurationId, Error, OfflineMode, Result,
    ServerClientImpl,
};

use super::{
    http_client::{ServerClient, WebsocketReader},
    NetworkError, NetworkResult,
};

const SERVER_HEARTBEAT: &str = "test message";

#[derive(Clone)]
pub enum CurrentModeOfflineReason {
    LockError,
    FailedToGetNewConfiguration,
    Initializing,
    WebsocketClosed,
    WebsocketError,
}

#[derive(Clone)]
pub enum CurrentMode {
    Online,
    Offline(CurrentModeOfflineReason),
}

pub(crate) struct LiveConfiguration {
    configuration: Arc<Mutex<Option<Configuration>>>,
    offline_mode: OfflineMode,
    current_mode: Arc<Mutex<CurrentMode>>,

    thread_termination_sender: std::sync::mpsc::Sender<()>,
    thread_handle: Option<JoinHandle<ThreadResult>>,
}

pub type ThreadResult = Result<()>;

pub enum ThreadStatus {
    Running,
    Finished(ThreadResult),
}

impl LiveConfiguration {
    pub fn get_configuration(&self) -> Result<Configuration> {
        match &*self.current_mode.lock()? {
            CurrentMode::Online => {
                match &*self.configuration.lock()? {
                    None => Err(Error::NetworkError(NetworkError::ContactToServerLost)),
                    // TODO: we do not want to clone here
                    Some(configuration) => Ok(configuration.clone()),
                }
            }
            CurrentMode::Offline(current_mode_offline_reason) => {
                match &self.offline_mode {
                    OfflineMode::Fail => {
                        Err(Error::NetworkError(NetworkError::ContactToServerLost))
                    }
                    OfflineMode::Cache => {
                        match &*self.configuration.lock()? {
                            None => Err(Error::NetworkError(NetworkError::ContactToServerLost)),
                            // TODO: we do not want to clone here
                            Some(configuration) => Ok(configuration.clone()),
                        }
                    }
                    OfflineMode::FallbackData(configuration) => Ok(configuration.clone()),
                }
            }
        }
    }

    pub fn get_thread_status(&mut self) -> ThreadStatus {
        let t = self.thread_handle.take();
        match t {
            Some(t) => {
                if t.is_finished() {
                    match t.join() {
                        Ok(r) => ThreadStatus::Finished(r),
                        Err(e) => {
                            if let Ok(panic_msg) = e.downcast::<String>() {
                                ThreadStatus::Finished(Err(Error::Other(format!(
                                    "Thread panicked: {}",
                                    panic_msg
                                ))))
                            } else {
                                ThreadStatus::Finished(Err(Error::Other(
                                    "Thread panicked".to_string(),
                                )))
                            }
                        }
                    }
                } else {
                    self.thread_handle = Some(t);
                    ThreadStatus::Running
                }
            }
            None => ThreadStatus::Finished(Err(Error::Other(
                "Thread already finished and the status was already requested.".to_string(),
            ))),
        }
    }

    pub fn get_current_mode(&self) -> Result<CurrentMode> {
        Ok(self.current_mode.lock()?.clone())
    }

    pub fn new_wait_until_online() {
        // Waits until the currentMode equals Online, so we have a first configuration
        // fetched from the server
        todo!("asdf")
    }

    pub fn new<T: ServerClient>(
        offline_mode: OfflineMode,
        server_client: T,
        configuration_id: ConfigurationId,
    ) -> Self {
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));
        let (thread_termination_sender, thread_handle) = Self::start_update_thread(
            server_client,
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        Self {
            configuration,
            offline_mode,
            thread_termination_sender,
            current_mode,
            thread_handle: Some(thread_handle),
        }
    }

    /// Whether the [`NetworkError`] will be permanent (it depends on static data) or we
    /// want to keep running the thread in case it eventually succeeds
    fn recoverable_error(error: NetworkError) -> NetworkResult<()> {
        match error {
            NetworkError::ReqwestError(_) => Ok(()),
            NetworkError::TungsteniteError(_) => Ok(()),
            NetworkError::ProtocolError => Ok(()),
            NetworkError::ContactToServerLost => Ok(()),
            // Make the match exhaustive, we need to pay attention to this classification
            NetworkError::UrlParseError(e) => Err(NetworkError::UrlParseError(e)),
            NetworkError::InvalidHeaderValue(e) => Err(NetworkError::InvalidHeaderValue(e)),
            NetworkError::CannotAcquireLock => Err(NetworkError::CannotAcquireLock),
        }
    }

    fn get_configuration_from_server<T: ServerClient>(
        server_client: &T,
        configuration_id: &ConfigurationId,
        configuration: Arc<Mutex<Option<Configuration>>>,
        current_mode: &CurrentMode,
    ) -> NetworkResult<CurrentMode> {
        server_client
            .get_configuration(&configuration_id)
            .and_then(|c| {
                Configuration::new(&configuration_id.environment_id, c)
                    .map_err(|_| NetworkError::ProtocolError)
            })
            .and_then(|cfg| {
                *configuration.lock()? = Some(cfg);
                Ok(CurrentMode::Online)
            })
            .or_else(|e| {
                Self::recoverable_error(e)?;

                if let CurrentMode::Offline(_) = current_mode {
                    Ok(current_mode.clone())
                } else {
                    Ok(CurrentMode::Offline(
                        CurrentModeOfflineReason::FailedToGetNewConfiguration,
                    ))
                }
            })
    }

    fn handle_websocket_payload<T: ServerClient>(
        utf8_bytes: tungstenite::Utf8Bytes,
        configuration: Arc<Mutex<Option<Configuration>>>,
        configuration_id: &ConfigurationId,
        server_client: &T,
        current_mode: &CurrentMode,
    ) -> std::result::Result<CurrentMode, NetworkError> {
        match utf8_bytes.as_str() {
            SERVER_HEARTBEAT => {
                if let CurrentMode::Offline(_) = current_mode {
                    Self::get_configuration_from_server(
                        server_client,
                        configuration_id,
                        configuration.clone(),
                        current_mode,
                    )
                } else {
                    Ok(current_mode.clone())
                }
            }
            _ => Self::get_configuration_from_server(
                server_client,
                configuration_id,
                configuration.clone(),
                current_mode,
            ),
        }
    }

    fn start_update_thread<T: ServerClient>(
        server_client: T,
        configuration_id: ConfigurationId,
        configuration: Arc<Mutex<Option<Configuration>>>,
        current_mode: Arc<Mutex<CurrentMode>>,
    ) -> (std::sync::mpsc::Sender<()>, JoinHandle<ThreadResult>) {
        let (thread_termination_sender, thread_termination_receiver) = std::sync::mpsc::channel();

        let t: JoinHandle<ThreadResult> = std::thread::spawn(move || {
            'outer: loop {
                // When want to have a configuration available asap.
                // FIXME: Add test case for race condition: if there is a configuration change
                //        happening between this 'get_configuration_from_server' and the ws
                //        handshake we are missing those changes. The ws is not yet connected,
                //        so it won't receive the 'config_update' message and the Configuration
                //        we got in this call doesn't include those changes.
                *current_mode.lock()? = Self::get_configuration_from_server(
                    &server_client,
                    &configuration_id,
                    configuration.clone(),
                    &current_mode.lock()?.clone(),
                )?;

                // Connect websocket
                let r = server_client.get_configuration_monitoring_websocket(&configuration_id);
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
                    match socket.read_msg() {
                        Ok(msg) => match msg {
                            tungstenite::Message::Text(utf8_bytes) => {
                                *current_mode.lock()? = Self::handle_websocket_payload(
                                    utf8_bytes,
                                    configuration.clone(),
                                    &configuration_id,
                                    &server_client,
                                    &*current_mode.lock()?,
                                )?;
                            }
                            tungstenite::Message::Close(close_frame) => {
                                *current_mode.lock()? =
                                    CurrentMode::Offline(CurrentModeOfflineReason::WebsocketClosed);
                                break 'inner;
                            }
                            _ => {
                                // Not specified in the WS protocol. We do nothing here.
                            }
                        },
                        Err(e) => {
                            *current_mode.lock()? =
                                CurrentMode::Offline(CurrentModeOfflineReason::WebsocketError);
                            break 'inner;
                        }
                    }
                }
            }

            Ok(())
        });

        (thread_termination_sender, t)
    }
}

#[cfg(test)]
mod tests {
    use crate::network::{configuration_http::{CurrentMode, CurrentModeOfflineReason, ThreadStatus}, http_client::{ServerClient, WebsocketReader}};

    use super::LiveConfiguration;

    #[test]
    fn test_happy_path() {
        struct WebsocketReaderMock {}
        impl WebsocketReader for WebsocketReaderMock {
            fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
                Ok(tungstenite::Message::text("asd".to_string()))
            }
        }
        struct ServerClientMock {}
        impl ServerClient for ServerClientMock {
            fn get_configuration(
                &self,
                _configuration_id: &crate::ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(crate::models::tests::configuration_feature1_enabled())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                collection: &crate::ConfigurationId,
            ) -> crate::NetworkResult<impl crate::network::http_client::WebsocketReader>
            {
                Ok(WebsocketReaderMock {})
            }
        }

        let server_client = ServerClientMock{};

        let configuration_id = crate::ConfigurationId::new("".into(), "".into(), "".into());
        let mut live_config =
            LiveConfiguration::new(crate::OfflineMode::Fail, server_client, configuration_id);

        // TODO: block thread from executing anything yet
        // Block beginning of get_configuration_from_server()
        
        // Expect we are in initializing state (no config)
        let config = live_config.get_configuration();
        assert!(matches!(config, Err(crate::errors::Error::MismatchType)));
        let thread_state = live_config.get_thread_status();
        assert!(matches!(thread_state, ThreadStatus::Running));
        let current_mode = live_config.get_current_mode();
        assert!(matches!(current_mode, Ok(CurrentMode::Offline(CurrentModeOfflineReason::Initializing))));

        // TODO: allow thread to start (unblock)
        // TODO: wait for thread to reach socket rx
        // Block in socket.read_msg()

        // Expect, we get a configuration and are Online / Running state
        let config = live_config.get_configuration();
        assert!(matches!(config, Ok(_)));
        let thread_state = live_config.get_thread_status();
        assert!(matches!(thread_state, ThreadStatus::Running));
        let current_mode = live_config.get_current_mode();
        assert!(matches!(current_mode, Ok(CurrentMode::Online)));

        // TODO: let thread continue (unblock)
        // TODO: simulate a socket msg (heart beat)
        // TODO: wait for thread to reach socket rx
        // Block in socket.read_msg()

        // Expect no change due to heartbeat:
        let config = live_config.get_configuration();
        assert!(matches!(config, Ok(_)));
        let thread_state = live_config.get_thread_status();
        assert!(matches!(thread_state, ThreadStatus::Running));
        let current_mode = live_config.get_current_mode();
        assert!(matches!(current_mode, Ok(CurrentMode::Online)));

        // TODO: let thread continue (unblock)
        // TODO: simulate a socket msg (you have mail message)
        // TODO: send new configuration via serverclient mock
        // Block in socket.read_msg()

        // Expect new configuration, and still running/online
        let config = live_config.get_configuration();
        assert!(matches!(config, Ok(_)));
        let thread_state = live_config.get_thread_status();
        assert!(matches!(thread_state, ThreadStatus::Running));
        let current_mode = live_config.get_current_mode();
        assert!(matches!(current_mode, Ok(CurrentMode::Online)));


        drop(live_config);
        // TODO: assert serverclient dropped (wait for rx queue message)
    }
}

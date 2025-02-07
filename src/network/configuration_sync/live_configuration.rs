use super::{Error, Result};
use crate::client::configuration::Configuration;
use crate::network::http_client::ServerClient;
use crate::ConfigurationId;
use std::sync::{Arc, Mutex};

use super::current_mode::{CurrentMode, CurrentModeOfflineReason};
use super::offline::OfflineMode;
use super::thread::UpdateThreadWorker;
use super::thread_handle::{ThreadHandle, ThreadStatus};

pub(crate) struct LiveConfiguration {
    configuration: Arc<Mutex<Option<Configuration>>>,
    offline_mode: OfflineMode,
    current_mode: Arc<Mutex<CurrentMode>>,

    update_thread: ThreadHandle,
}

impl LiveConfiguration {
    pub fn new<T: ServerClient>(
        offline_mode: OfflineMode,
        server_client: T,
        configuration_id: ConfigurationId,
    ) -> Self {
        let configuration = Arc::new(Mutex::new(None));
        let current_mode = Arc::new(Mutex::new(CurrentMode::Offline(
            CurrentModeOfflineReason::Initializing,
        )));

        let mut worker = UpdateThreadWorker::new(
            server_client,
            configuration_id,
            configuration.clone(),
            current_mode.clone(),
        );

        let update_thread =
            ThreadHandle::new(move |terminator_receiver| worker.run(terminator_receiver));

        Self {
            configuration,
            offline_mode,
            update_thread,
            current_mode,
        }
    }

    pub(crate) fn get_configuration(&self) -> Result<Configuration> {
        match &*self.current_mode.lock()? {
            CurrentMode::Online => {
                match &*self.configuration.lock()? {
                    // We store the configuration retrieved from the server into the Arc<Mutex> before switching the flag to Online
                    None => unreachable!(),
                    // TODO: we do not want to clone here
                    Some(configuration) => Ok(configuration.clone()),
                }
            }
            CurrentMode::Offline(current_mode_offline_reason) => {
                match &self.offline_mode {
                    OfflineMode::Fail => Err(Error::Offline(current_mode_offline_reason.clone())),
                    OfflineMode::Cache => {
                        match &*self.configuration.lock()? {
                            None => Err(Error::ConfigurationNotYetAvailable),
                            // TODO: we do not want to clone here
                            Some(configuration) => Ok(configuration.clone()),
                        }
                    }
                    OfflineMode::FallbackData(configuration) => Ok(configuration.clone()),
                }
            }
        }
    }

    pub(crate) fn get_thread_status(&mut self) -> ThreadStatus {
        self.update_thread.get_thread_status()
    }

    pub(crate) fn get_current_mode(&self) -> Result<CurrentMode> {
        Ok(self.current_mode.lock()?.clone())
    }
}

#[cfg(test)]
mod tests {

    use std::sync::mpsc;

    use crate::{
        models::tests::configuration_property1_enabled,
        network::{configuration_sync::thread::SERVER_HEARTBEAT, http_client::WebsocketReader},
    };

    use super::*;

    #[test]
    fn test_happy_path() {
        struct WebsocketReaderMock {
            rx: mpsc::Receiver<tungstenite::Message>,
            tx: mpsc::Sender<()>,
        }
        impl WebsocketReader for WebsocketReaderMock {
            fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
                self.tx.send(());
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
                _configuration_id: &crate::ConfigurationId,
            ) -> crate::NetworkResult<crate::models::ConfigurationJson> {
                Ok(self.rx.recv().unwrap())
            }

            fn get_configuration_monitoring_websocket(
                &self,
                _collection: &crate::ConfigurationId,
            ) -> crate::NetworkResult<impl crate::network::http_client::WebsocketReader>
            {
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
            LiveConfiguration::new(crate::OfflineMode::Fail, server_client, configuration_id);

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
            assert!(matches!(config_result, Ok(_)));
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
            assert!(matches!(config_result, Ok(_)));
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
            assert!(matches!(r, Err(RecvError)), "{:?}", r)
        }
    }

    #[test]
    fn test_wrong_url() {
        // TODO: If we provide a wrong URL to the server
    }

    #[test]
    fn test_wait_for_initial_configuration() {
        // TODO (or not): A way to create a LiveConfiguration object and wait until the first Configuration is available
    }

    #[test]
    fn test_when_thread_stopped_we_need_to_be_offline() {
        // TODO
    }
}

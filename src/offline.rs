use crate::client::configuration::Configuration;

/// Defines the behaviour of the client while the connection to the server
/// is lost. In all cases the client will keep trying to reconnect forever.
#[derive(Debug)]
pub enum OfflineMode {
    /// Returns errors when requesting features or evaluating them
    Fail,

    /// Return features and values from the latests configuration available
    Cache,

    /// Use the provided configuration.
    FallbackData(Configuration),
}

#[derive(Debug)]
pub(crate) struct OperationMode {
    connected: bool,
    offline_mode: OfflineMode,
}

impl OperationMode {
    pub fn new(offline_mode: OfflineMode, connected: bool) -> Self {
        Self {
            connected,
            offline_mode,
        }
    }

    pub fn i_managed_to_connect(&mut self) {
        self.connected = true;
    }

    pub fn i_lost_connection(&mut self) {
        self.connected = false;
    }
}

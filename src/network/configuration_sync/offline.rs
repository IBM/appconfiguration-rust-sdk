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
    FallbackData(Configuration), // FIXME: The public type "should" be ConfigurationJSON, or the user should just provide a JSON file (same input as the Offline client)
}

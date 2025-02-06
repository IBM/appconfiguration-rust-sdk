#[derive(Clone, Debug)]
pub(crate) enum CurrentMode {
    Online,
    Offline(CurrentModeOfflineReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CurrentModeOfflineReason {
    LockError,
    FailedToGetNewConfiguration,
    Initializing,
    WebsocketClosed,
    WebsocketError,
    ConfigurationDataInvalid,
}

impl std::fmt::Display for CurrentModeOfflineReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CurrentModeOfflineReason::LockError => write!(f, "LockError"),
            CurrentModeOfflineReason::FailedToGetNewConfiguration => {
                write!(f, "FailedToGetNewConfiguration")
            }
            CurrentModeOfflineReason::Initializing => write!(f, "Initializing"),
            CurrentModeOfflineReason::WebsocketClosed => write!(f, "WebsocketClosed"),
            CurrentModeOfflineReason::WebsocketError => write!(f, "WebsocketError"),
            CurrentModeOfflineReason::ConfigurationDataInvalid => {
                write!(f, "ConfigurationDataInvalid")
            }
        }
    }
}

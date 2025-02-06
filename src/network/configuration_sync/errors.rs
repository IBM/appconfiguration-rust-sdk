use std::sync::PoisonError;

use thiserror::Error;

use super::current_mode::CurrentModeOfflineReason;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub(crate) enum Error {
    #[error("Cannot acquire lock")]
    CannotAcquireLock,

    #[error("Connection to server lost: {0}")]
    Offline(CurrentModeOfflineReason),

    #[error("Configuration is not yet available (try again later)")]
    ConfigurationNotYetAvailable,

    #[error("Thread failed with internal error: {0}")]
    ThreadInternalError(String),

    #[error("{0}")]
    UnrecoverableError(String),
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_value: PoisonError<T>) -> Self {
        Error::CannotAcquireLock
    }
}

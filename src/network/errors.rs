use std::sync::PoisonError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    TungsteniteError(#[from] tungstenite::Error),

    #[error("Protocol error. Unexpected data received from server")]
    ProtocolError,

    #[error("Cannot parse '{0}' as URL")]
    UrlParseError(String),

    #[error("Invalid header value for '{0}'")]
    InvalidHeaderValue(String),

    #[error("Cannot acquire lock")]
    CannotAcquireLock,

    #[error("Contact to server lost")]
    ContactToServerLost,
}

impl<T> From<PoisonError<T>> for NetworkError {
    fn from(_value: PoisonError<T>) -> Self {
        NetworkError::CannotAcquireLock
    }
}

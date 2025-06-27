use std::sync::PoisonError;

use thiserror::Error;

use crate::network::errors::NetworkError;
use crate::segment_evaluation::errors::SegmentEvaluationError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Cannot acquire snapshot lock")]
    CannotAcquireLock,

    #[error("Feature '{feature_id}' does not exist in environment '{environment_id}' and collection '{collection_id}'")]
    FeatureDoesNotExist {
        collection_id: String,
        environment_id: String,
        feature_id: String,
    },

    #[error("Property '{property_id}' does not exist in environment '{environment_id}' and collection '{collection_id}'")]
    PropertyDoesNotExist {
        collection_id: String,
        environment_id: String,
        property_id: String,
    },

    #[error("Inner type cannot be converted to requested type")]
    MismatchType,

    #[error(transparent)]
    TungsteniteError(#[from] tungstenite::Error),

    #[error("Protocol error. Unexpected data received from server")]
    ProtocolError(String),

    #[error(transparent)]
    DeserializationError(#[from] DeserializationError),

    #[error("Client is not configured")]
    ClientNotConfigured,

    #[error(transparent)]
    ConfigurationAccessError(#[from] ConfigurationAccessError),

    #[error("Failed to evaluate entity: {0}")]
    EntityEvaluationError(EntityEvaluationError),

    #[error(transparent)]
    NetworkError(#[from] NetworkError),

    #[error(transparent)]
    LiveConfigurationError(#[from] LiveConfigurationError),

    #[error("Failed to record evaluation event for metering")]
    MeteringError,

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct EntityEvaluationError(pub(crate) SegmentEvaluationError);

impl From<SegmentEvaluationError> for Error {
    fn from(value: SegmentEvaluationError) -> Self {
        Self::EntityEvaluationError(EntityEvaluationError(value))
    }
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_value: PoisonError<T>) -> Self {
        Error::CannotAcquireLock
    }
}

/// An error that can be returned when deserializing data.
#[derive(Debug, Error)]
#[error("Cannot deserialize string '{string}': {source}")]
pub struct DeserializationError {
    pub string: String,
    pub source: DeserializationErrorKind,
}

/// Additional information for [`DeserializationError`] error
#[derive(Debug, Error)]
pub enum DeserializationErrorKind {
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum ConfigurationAccessError {
    #[error("Error acquiring index cache lock")]
    LockAcquisitionError,

    #[error(
        "Environment '{environment_id}' indicated as key not found in the configuration instance"
    )]
    EnvironmentNotFound { environment_id: String },

    #[error("Feature `{feature_id}` not found.")]
    FeatureNotFound { feature_id: String },

    #[error("Property `{property_id}` not found.")]
    PropertyNotFound { property_id: String },

    #[error("Missing segments for resource '{resource_id}'")]
    MissingSegments { resource_id: String },
}

impl<T> From<PoisonError<T>> for ConfigurationAccessError {
    fn from(_value: PoisonError<T>) -> Self {
        ConfigurationAccessError::LockAcquisitionError
    }
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct LiveConfigurationError(crate::network::live_configuration::Error);

impl From<crate::network::live_configuration::Error> for Error {
    fn from(value: crate::network::live_configuration::Error) -> Self {
        Self::LiveConfigurationError(LiveConfigurationError(value))
    }
}

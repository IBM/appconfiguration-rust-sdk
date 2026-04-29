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

    #[error("Secret property '{property_id}' is not configured as type SECRETREF")]
    PropertyIsNotSecretRef { property_id: String },

    #[error("Secret manager is not configured for secret property '{property_id}'")]
    SecretManagerNotConfigured { property_id: String },

    #[error("Secret reference for property '{property_id}' did not contain an 'id' field")]
    SecretReferenceIdMissing { property_id: String },

    #[error("Secret provider failed for property '{property_id}': {message}")]
    SecretProviderError {
        property_id: String,
        message: String,
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

    #[error(transparent)]
    ConfigurationDataError(#[from] ConfigurationDataError),

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
pub enum ConfigurationDataError {
    #[error("Environment '{0}' not found")]
    EnvironmentNotFound(String),

    #[error("Collection '{0}' not found")]
    CollectionNotFound(String),

    #[error("Improper/Missing collections in configuration")]
    MissingCollections,

    #[error("Feature `{0}` not found.")]
    FeatureNotFound(String),

    #[error("Property `{0}` not found.")]
    PropertyNotFound(String),

    #[error("Missing segments for resource '{0}'")]
    MissingSegments(String),

    #[error("Improper collection format in resource '{0}'")]
    InvalidResourceCollections(String),
}

#[derive(Debug, Error)]
pub enum ConfigurationAccessError {
    #[error("Error acquiring index cache lock")]
    LockAcquisitionError,
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

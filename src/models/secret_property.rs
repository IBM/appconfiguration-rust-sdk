use crate::entity::Entity;
use crate::errors::Result;
use crate::models::PropertySnapshot;
use crate::{Error, Property, Value};

/// Resolves a `SECRETREF` property into the final secret string using a user-provided secret manager.
///
/// This mirrors the Node SDK shape where [`ConfigurationHandler.getSecret()`](appconfiguration-node-sdk/lib/configurations/ConfigurationHandler.js:543)
/// returns a dedicated secret-property wrapper on top of a normal property evaluation.
pub trait SecretManager: Send + Sync {
    /// Fetch the secret value for the given secret identifier.
    fn get_secret(&self, secret_id: &str) -> Result<String>;
}

/// Snapshot wrapper for a secret-reference property.
///
/// The wrapped property is first evaluated normally. Its resulting value must be a string that
/// contains the secret identifier. That identifier is then resolved through the configured
/// [`SecretManager`].
#[derive(Debug)]
pub struct SecretPropertySnapshot {
    property: PropertySnapshot,
    property_id: String,
}

impl SecretPropertySnapshot {
    pub(crate) fn new(property: PropertySnapshot, property_id: String) -> Self {
        Self {
            property,
            property_id,
        }
    }

    /// Resolve the secret value for the given entity using the provided secret manager.
    pub fn get_current_value(
        &self,
        entity: &impl Entity,
        secret_manager: &impl SecretManager,
    ) -> Result<String> {
        let evaluated = self.property.get_current_value(entity)?;
        let secret_id = match evaluated.value {
            Value::String(secret_id) if !secret_id.is_empty() => secret_id,
            _ => {
                return Err(Error::SecretReferenceIdMissing {
                    property_id: self.property_id.clone(),
                });
            }
        };

        secret_manager
            .get_secret(&secret_id)
            .map_err(|error| Error::SecretProviderError {
                property_id: self.property_id.clone(),
                message: error.to_string(),
            })
    }

    /// Returns the underlying property name.
    pub fn get_name(&self) -> Result<String> {
        self.property.get_name()
    }

    /// Returns the property id associated with this secret wrapper.
    pub fn get_property_id(&self) -> &str {
        &self.property_id
    }

    /// Returns the underlying evaluated property snapshot.
    pub fn property(&self) -> &PropertySnapshot {
        &self.property
    }
}

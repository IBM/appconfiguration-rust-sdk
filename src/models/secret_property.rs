// Copyright 2026 IBM Corp. All Rights Reserved.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

//       http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::entity::Entity;
use crate::errors::Result;
use crate::models::PropertySnapshot;
use crate::{Error, Property, Value};

pub trait SecretManager: Send + Sync {
    /// Fetch the secret value for the given secret identifier.
    fn get_secret(&self, secret_id: &str) -> Result<String>;
}

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

    pub fn get_property_name(&self) -> Result<String> {
        self.property.get_property_name()
    }

    /// Returns the property id associated with this secret wrapper.
    pub fn get_property_id(&self) -> &str {
        &self.property_id
    }
}

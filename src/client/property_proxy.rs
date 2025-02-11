// (C) Copyright IBM Corp. 2024.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::Property;

use super::property_snapshot::PropertySnapshot;
use super::AppConfigurationClient;
use crate::value::Value;
use crate::Entity;

/// Provides live-updated data for a given [`Property`].
pub struct PropertyProxy<'a> {
    client: &'a dyn AppConfigurationClient,
    property_id: String,
}

impl<'a> PropertyProxy<'a> {
    pub(crate) fn new(client: &'a dyn AppConfigurationClient, property_id: String) -> Self {
        Self {
            client,
            property_id,
        }
    }

    /// Take a snapshot of this proxied property
    pub fn snapshot(&self) -> crate::errors::Result<PropertySnapshot> {
        self.client.get_property(&self.property_id)
    }
}

impl Property for PropertyProxy<'_> {
    fn get_name(&self) -> crate::errors::Result<String> {
        self.client.get_property(&self.property_id)?.get_name()
    }

    fn get_value(&self, entity: &impl Entity) -> crate::errors::Result<Value> {
        self.client
            .get_property(&self.property_id)?
            .get_value(entity)
    }

    fn get_value_into<T: TryFrom<Value, Error = crate::Error>>(
        &self,
        entity: &impl Entity,
    ) -> crate::errors::Result<T> {
        self.client
            .get_property(&self.property_id)?
            .get_value_into(entity)
    }
}

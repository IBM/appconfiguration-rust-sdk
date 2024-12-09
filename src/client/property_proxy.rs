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

use crate::entity::Entity;

use super::property_snapshot::PropertySnapshot;
use super::AppConfigurationClient;

pub struct PropertyProxy<'a> {
    client: &'a AppConfigurationClient,
    property_id: String,
}

impl<'a> PropertyProxy<'a> {
    pub(crate) fn new(client: &'a AppConfigurationClient, property_id: String) -> Self {
        Self {
            client,
            property_id,
        }
    }

    pub fn snapshot(&self) -> crate::errors::Result<PropertySnapshot> {
        self.client.get_property(&self.property_id)
    }
}

impl<'a> Property for PropertyProxy<'a> {
    fn get_id(&self) -> &str {
        &self.property_id
    }

    fn get_name(&self) -> crate::errors::Result<String> {
        self.client.get_property(&self.property_id)?.get_name()
    }

    fn get_data_type(&self) -> crate::errors::Result<crate::models::ValueKind> {
        self.client.get_property(&self.property_id)?.get_data_type()
    }

    fn get_value_default(&self) -> crate::errors::Result<crate::models::ConfigValue> {
        self.client
            .get_property(&self.property_id)?
            .get_value_default()
    }

    fn get_value(&self, entity: &impl Entity) -> crate::errors::Result<super::value::Value> {
        self.client
            .get_property(&self.property_id)?
            .get_value(entity)
    }
}

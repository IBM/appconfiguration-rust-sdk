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

use std::sync::{Arc, Mutex};

use crate::{
    client::cache::ConfigurationSnapshot, models,
    segment_evaluation::find_applicable_segment_rule_for_entity,
};

use crate::entity::Entity;

use crate::errors::ConfigurationAccessError;

const MISSING_PROPERTY_ERROR_MSG: &str = "The property should exist in the index. It should have been validated in `AppConfigurationClient::get_property()`.";

/// A property in a collection and environment. Use the `get_property()`
/// method of the `AppConfigurationClient` to create instances of properties.
#[derive(Debug)]
pub struct PropertyProxy {
    configuration_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
    property_id: String,
}

impl PropertyProxy {
    pub(crate) fn new(
        configuration_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
        property_id: String,
    ) -> Self {
        PropertyProxy {
            configuration_snapshot,
            property_id,
        }
    }

    /// Returns the name of the property.
    pub fn get_name(&self) -> String {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_property(&self.property_id)
            .expect(MISSING_PROPERTY_ERROR_MSG)
            .name
            .clone()
    }

    /// Returns the value of the property as a `models::ConfigValue`.
    pub fn get_value(&self) -> models::ConfigValue {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_property(&self.property_id)
            .expect(MISSING_PROPERTY_ERROR_MSG)
            .value
            .clone()
    }

    /// Returns the id of the property.
    pub fn get_id(&self) -> String {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_property(&self.property_id)
            .expect(MISSING_PROPERTY_ERROR_MSG)
            .property_id
            .clone()
    }

    /// Returns the data type as a member of the `models::ValueKind` enumeration.
    pub fn get_data_type(&self) -> models::ValueKind {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_property(&self.property_id)
            .expect(MISSING_PROPERTY_ERROR_MSG)
            .kind
    }

    /// Gets the `Some(data_format)` if the feature data type is
    /// `models::ValueKind::STRING`, or `None` otherwise.
    pub fn get_data_format(&self) -> Option<String> {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_property(&self.property_id)
            .expect(MISSING_PROPERTY_ERROR_MSG)
            .format
            .clone()
    }

    /// Returns the targeting rules for the property. I.e.: what value to
    /// associate with an entity, and under what circumstances it applies.
    pub fn get_targeting_rules(&self) -> Vec<models::TargetingRule> {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_property(&self.property_id)
            .expect(MISSING_PROPERTY_ERROR_MSG)
            .segment_rules
            .clone()
    }

    /// Evaluates the property for `entity` and returns the evaluation as a
    /// `models::ConfigValue`.
    pub fn get_current_value(&self, entity: &impl Entity) -> models::ConfigValue {
        self.evaluate_feature_for_entity(entity)
    }

    fn evaluate_feature_for_entity(&self, entity: &impl Entity) -> models::ConfigValue {
        let segment_rule = find_applicable_segment_rule_for_entity(
            &self
                .configuration_snapshot
                .lock()
                .unwrap_or_else(|e| panic!("Failed to acquire configuration snapshot lock: {e}"))
                .segments,
            self.get_targeting_rules().into_iter(),
            entity,
        );
        if let Some(segment_rule) = segment_rule {
            self.resolve_value(&segment_rule)
        } else {
            self.get_value()
        }
    }

    fn resolve_value(&self, segment_rule: &models::TargetingRule) -> models::ConfigValue {
        if segment_rule.value.is_default() {
            self.get_value()
        } else {
            segment_rule.value.clone()
        }
    }
}

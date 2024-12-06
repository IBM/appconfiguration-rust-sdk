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

use std::sync::Arc;
use std::{io::Cursor, sync::Mutex};

use murmur3::murmur3_32;

use crate::{
    client::cache::ConfigurationSnapshot, models,
    segment_evaluation::find_applicable_segment_rule_for_entity,
};

use crate::entity::Entity;

use crate::errors::ConfigurationAccessError;

const MISSING_FEATURE_ERROR_MSG: &str = "The feature should exist in the configuration_snapshot. It should have been validated in `AppConfigurationClient::get_feature()`.";

/// A feature in a collection and environment. Use the `get_feature()`
/// method of the `AppConfigurationClient` to create instances of features.
#[derive(Debug)]
pub struct FeatureProxy {
    configuration_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
    feature_id: String,
}

impl FeatureProxy {
    pub(crate) fn new(
        configuration_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
        feature_id: String,
    ) -> Self {
        FeatureProxy {
            configuration_snapshot,
            feature_id,
        }
    }

    /// Returns the name of the feature.
    pub fn get_name(&self) -> String {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .name
            .clone()
    }

    /// Returns the disable value as a `models::ConfigValue`.
    pub fn get_disabled_value(&self) -> models::ConfigValue {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .disabled_value
            .clone()
    }

    /// Returns the enabled value as a `models::ConfigValue`.
    pub fn get_enabled_value(&self) -> models::ConfigValue {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .enabled_value
            .clone()
    }

    /// Returns the id of the feature.
    pub fn get_id(&self) -> String {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .feature_id
            .clone()
    }

    /// Returns the data type as a member of the `models::ValueKind` enumeration.
    pub fn get_data_type(&self) -> models::ValueKind {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .kind
    }

    /// Gets the `Some(data_format)` if the feature data type is
    /// `models::ValueKind::STRING`, or `None` otherwise.
    pub fn get_data_format(&self) -> Option<String> {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .format
            .clone()
    }

    /// Returns the rollout peArcentage as a positive integer.
    pub fn get_rollout_percentage(&self) -> u32 {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .rollout_percentage
    }

    /// Returns the targeting rules for the feature. I.e.: what value to
    /// associate with an entity, under what ciArcumnstances, and how frequent
    /// it applies.
    pub fn get_targeting_rules(&self) -> Vec<models::TargetingRule> {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .segment_rules
            .clone()
    }

    /// Returns if the feature is enabled or not.
    pub fn is_enabled(&self) -> bool {
        self.configuration_snapshot
            .lock()
            .unwrap_or_else(|_| panic!("{}", ConfigurationAccessError::LockAcquisitionError))
            .get_feature(&self.feature_id)
            .expect(MISSING_FEATURE_ERROR_MSG)
            .enabled
    }

    /// Evaluates the feature for `entity` and returns the evaluation as a
    /// `models::ConfigValue`.
    pub fn get_current_value(&self, entity: &impl Entity) -> models::ConfigValue {
        if !self.is_enabled() {
            self.get_disabled_value()
        } else {
            self.evaluate_feature_for_entity(entity)
        }
    }

    fn evaluate_feature_for_entity(&self, entity: &impl Entity) -> models::ConfigValue {
        let tag = format!("{}:{}", entity.get_id(), self.get_id());

        if self.get_targeting_rules().len() == 0 && entity.get_attributes().len() == 0 {
            // TODO rollout percentage evaluation
        }

        let segment_rule = find_applicable_segment_rule_for_entity(
            &self
                .configuration_snapshot
                .lock()
                .unwrap_or_else(|e| panic!("Failed to acquire configuration snapshot lock: {e}"))
                .segments,
            self.get_targeting_rules().into_iter(),
            entity,
        ).unwrap_or_else(|e| panic!("Failed to evaluate segment rules: {e}"));
        if let Some(segment_rule) = segment_rule {
            let rollout_percentage = self.resolve_rollout_percentage(&segment_rule);
            if rollout_percentage == 100 || random_value(&tag) < rollout_percentage {
                self.resolve_enabled_value(&segment_rule)
            } else {
                self.get_disabled_value()
            }
        } else {
            let rollout_percentage = self.get_rollout_percentage();
            if rollout_percentage == 100 || random_value(&tag) < rollout_percentage {
                self.get_enabled_value()
            } else {
                self.get_disabled_value()
            }
        }
    }

    fn resolve_rollout_percentage(&self, segment_rule: &models::TargetingRule) -> u32 {
        let missing_rollout_msg = "Rollout velue is missing.";
        let rollout_value = segment_rule
            .rollout_percentage
            .as_ref()
            .expect(missing_rollout_msg);
        if rollout_value.is_default() {
            self.get_rollout_percentage()
        } else {
            u32::try_from(rollout_value.as_u64().expect(missing_rollout_msg))
                .expect("Invalid rollout value.")
        }
    }

    fn resolve_enabled_value(&self, segment_rule: &models::TargetingRule) -> models::ConfigValue {
        if segment_rule.value.is_default() {
            self.get_enabled_value()
        } else {
            segment_rule.value.clone()
        }
    }
}

pub fn random_value(v: &str) -> u32 {
    let max_hash = u32::MAX;
    (f64::from(hash(v)) / f64::from(max_hash) * 100.0) as u32
}

fn hash(v: &str) -> u32 {
    murmur3_32(&mut Cursor::new(v), 0).expect("Cannot hash the value.")
}

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

use std::collections::{HashMap, HashSet};

use crate::errors::{ConfigurationAccessError, Result};
use crate::models::{ConfigurationJson, Feature, Property, Segment, TargetingRule};
use crate::segment_evaluation::SegmentRules;
use crate::Error;

use super::feature_snapshot::FeatureSnapshot;
use super::property_snapshot::PropertySnapshot;

/// Represents all the configuration data needed for the client to perform
/// feature/propery evaluation.
/// It contains a subset of models::ConfigurationJson, adding indexing.
#[derive(Debug, Default)]
pub(crate) struct Configuration {
    pub(crate) features: HashMap<String, Feature>,
    pub(crate) properties: HashMap<String, Property>,
    pub(crate) segments: HashMap<String, Segment>,
}

impl Configuration {
    /// Constructs the Configuration, by consuming and filtering data in exchange format
    pub fn new(environment_id: &str, configuration: ConfigurationJson) -> Result<Self> {
        let environment = configuration
            .environments
            .into_iter()
            .find(|e| e.environment_id == environment_id)
            .ok_or(ConfigurationAccessError::EnvironmentNotFound {
                environment_id: environment_id.to_string(),
            })?;
        // FIXME: why not filtering for collection here?

        let mut features = HashMap::new();
        for mut feature in environment.features {
            feature.segment_rules.sort_by(|a, b| a.order.cmp(&b.order));
            features.insert(feature.feature_id.clone(), feature);
        }

        let mut properties = HashMap::new();
        for mut property in environment.properties {
            property.segment_rules.sort_by(|a, b| a.order.cmp(&b.order));
            properties.insert(property.property_id.clone(), property);
        }

        let mut segments = HashMap::new();
        for segment in configuration.segments {
            segments.insert(segment.segment_id.clone(), segment.clone());
        }
        Ok(Configuration {
            features,
            properties,
            segments,
        })
    }

    pub fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        // Get the feature from the snapshot
        let feature = self.features.get(feature_id).ok_or_else(|| {
            Error::ConfigurationAccessError(ConfigurationAccessError::FeatureNotFound {
                feature_id: feature_id.to_string(),
            })
        })?;

        // Get the segment rules that apply to this feature
        let segments = self.get_segments_for_segment_rules(&feature.segment_rules);

        // Integrity DB check: all segment_ids should be available in the snapshot
        if feature.segment_rules.len() != segments.len() {
            return Err(ConfigurationAccessError::MissingSegments {
                resource_id: feature_id.to_string(),
            }
            .into());
        }

        let segment_rules =
            SegmentRules::new(segments, feature.segment_rules.clone(), feature.kind);
        let enabled_value = (feature.kind, feature.enabled_value.clone()).try_into()?;
        let disabled_value = (feature.kind, feature.disabled_value.clone()).try_into()?;
        Ok(FeatureSnapshot::new(
            feature.enabled,
            enabled_value,
            disabled_value,
            feature.rollout_percentage,
            &feature.name,
            feature_id,
            segment_rules,
        ))
    }

    pub fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        // Get the property from the snapshot
        let property = self.properties.get(property_id).ok_or_else(|| {
            Error::ConfigurationAccessError(ConfigurationAccessError::PropertyNotFound {
                property_id: property_id.to_string(),
            })
        })?;

        // Get the segment rules that apply to this property
        let segments = self.get_segments_for_segment_rules(&property.segment_rules);

        // Integrity DB check: all segment_ids should be available in the snapshot
        if property.segment_rules.len() != segments.len() {
            return Err(ConfigurationAccessError::MissingSegments {
                resource_id: property_id.to_string(),
            }
            .into());
        }

        let value = (property.kind, property.value.clone()).try_into()?;
        let segment_rules =
            SegmentRules::new(segments, property.segment_rules.clone(), property.kind);
        Ok(PropertySnapshot::new(value, segment_rules, &property.name))
    }

    /// Returns a mapping of segment ID to `Segment` for all segments referenced
    /// by the given `segment_rules`.
    pub(crate) fn get_segments_for_segment_rules(
        &self,
        segment_rules: &[TargetingRule],
    ) -> HashMap<String, Segment> {
        let referenced_segment_ids = segment_rules
            .iter()
            .flat_map(|targeting_rule| {
                targeting_rule
                    .rules
                    .iter()
                    .flat_map(|segment| &segment.segments)
            })
            .cloned()
            .collect::<HashSet<String>>();

        self.segments
            .iter()
            .filter(|&(key, _)| referenced_segment_ids.contains(key))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::Error;
    use crate::models::tests::example_configuration_enterprise;
    use crate::models::ConfigurationJson;

    use rstest::*;

    #[rstest]
    fn test_filter_configurations(example_configuration_enterprise: ConfigurationJson) {
        let result =
            Configuration::new("does_for_sure_not_exist", example_configuration_enterprise);
        assert!(result.is_err());

        assert!(matches!(
                result.unwrap_err(),
                Error::ConfigurationAccessError(ref e)
                if matches!(e, ConfigurationAccessError::EnvironmentNotFound { ref environment_id} if environment_id == "does_for_sure_not_exist")));
    }
}

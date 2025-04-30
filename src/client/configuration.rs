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
#[derive(Debug, Default, Clone)]
pub(crate) struct Configuration {
    features: HashMap<String, (Feature, SegmentRules)>,
    properties: HashMap<String, (Property, SegmentRules)>,
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

        let features = environment
            .features
            .into_iter()
            .map(|mut feature| {
                feature.segment_rules.sort_by(|a, b| a.order.cmp(&b.order));

                // Get the segment rules that apply to this feature
                let segments = Self::get_segments_for_segment_rules(
                    &configuration.segments,
                    &feature.segment_rules,
                );

                // Integrity DB check: all segment_ids should be available in the snapshot
                if feature.segment_rules.len() != segments.len() {
                    return Err(ConfigurationAccessError::MissingSegments {
                        resource_id: feature.feature_id.to_string(),
                    }
                    .into());
                }

                let segment_rules =
                    SegmentRules::new(segments, feature.segment_rules.clone(), feature.kind);

                Ok((feature.feature_id.clone(), (feature, segment_rules)))
            })
            .collect::<Result<_>>()?;

        let properties = environment
            .properties
            .into_iter()
            .map(|mut property| {
                property.segment_rules.sort_by(|a, b| a.order.cmp(&b.order));

                // Get the segment rules that apply to this property
                let segments = Self::get_segments_for_segment_rules(
                    &configuration.segments,
                    &property.segment_rules,
                );

                // Integrity DB check: all segment_ids should be available in the snapshot
                if property.segment_rules.len() != segments.len() {
                    return Err(ConfigurationAccessError::MissingSegments {
                        resource_id: property.property_id.to_string(),
                    }
                    .into());
                }

                let segment_rules =
                    SegmentRules::new(segments, property.segment_rules.clone(), property.kind);
                Ok((property.property_id.clone(), (property, segment_rules)))
            })
            .collect::<Result<_>>()?;

        Ok(Configuration {
            features,
            properties,
        })
    }

    pub fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        // Get the feature from the snapshot
        let (feature, segment_rules) = self.features.get(feature_id).ok_or_else(|| {
            Error::ConfigurationAccessError(ConfigurationAccessError::FeatureNotFound {
                feature_id: feature_id.to_string(),
            })
        })?;

        let enabled_value = (feature.kind, feature.enabled_value.clone()).try_into()?;
        let disabled_value = (feature.kind, feature.disabled_value.clone()).try_into()?;
        Ok(FeatureSnapshot::new(
            feature.enabled,
            enabled_value,
            disabled_value,
            feature.rollout_percentage,
            &feature.name,
            feature_id,
            segment_rules.clone(),
        ))
    }

    pub fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        // Get the property from the snapshot
        let (property, segment_rules) = self.properties.get(property_id).ok_or_else(|| {
            Error::ConfigurationAccessError(ConfigurationAccessError::PropertyNotFound {
                property_id: property_id.to_string(),
            })
        })?;

        let value = (property.kind, property.value.clone()).try_into()?;
        Ok(PropertySnapshot::new(
            value,
            segment_rules.clone(),
            &property.name,
        ))
    }

    /// Returns a mapping of segment ID to `Segment` for all segments referenced
    /// by the given `segment_rules`.
    fn get_segments_for_segment_rules(
        segments: &[Segment],
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

        segments
            .iter()
            .filter(|&segment| referenced_segment_ids.contains(&segment.segment_id))
            .map(|segment| (segment.segment_id.clone(), segment.clone()))
            .collect()
    }

    pub(crate) fn get_feature_ids(&self) -> Vec<&String> {
        self.features.keys().collect()
    }

    pub(crate) fn get_property_ids(&self) -> Vec<&String> {
        self.properties.keys().collect()
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

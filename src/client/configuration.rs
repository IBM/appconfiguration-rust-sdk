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

use crate::errors::Result;
use crate::models::{ConfigurationJson, Feature, Property, Segment, SegmentRule};
use crate::segment_evaluation::TargetingRules;
use crate::ConfigurationDataError;

use super::feature_snapshot::FeatureSnapshot;
use super::property_snapshot::PropertySnapshot;
use super::ConfigurationProvider;

/// Represents all the configuration data needed for the client to perform
/// feature/propery evaluation.
/// It contains a subset of models::ConfigurationJson, adding indexing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Configuration {
    pub(crate) features: HashMap<String, (Feature, TargetingRules)>,
    pub(crate) properties: HashMap<String, (Property, TargetingRules)>,
}

impl Configuration {
    /// Constructs the Configuration, by consuming and filtering data in exchange format
    pub fn new(
        environment_id: &str,
        configuration: ConfigurationJson,
    ) -> std::result::Result<Self, ConfigurationDataError> {
        let environment = configuration
            .environments
            .into_iter()
            .find(|e| e.environment_id == environment_id)
            .ok_or(ConfigurationDataError::EnvironmentNotFound(
                environment_id.to_string(),
            ))?;
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
                    return Err(ConfigurationDataError::MissingSegments(
                        feature.feature_id.to_string(),
                    ));
                }

                let segment_rules =
                    TargetingRules::new(segments, feature.segment_rules.clone(), feature.r#type);

                Ok((feature.feature_id.clone(), (feature, segment_rules)))
            })
            .collect::<std::result::Result<_, _>>()?;

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
                    return Err(ConfigurationDataError::MissingSegments(
                        property.property_id.to_string(),
                    ));
                }

                let segment_rules =
                    TargetingRules::new(segments, property.segment_rules.clone(), property.r#type);
                Ok((property.property_id.clone(), (property, segment_rules)))
            })
            .collect::<std::result::Result<_, _>>()?;

        Ok(Configuration {
            features,
            properties,
        })
    }

    pub fn from_file(filepath: &std::path::Path, environment_id: &str) -> Result<Self> {
        let configuration = ConfigurationJson::new(filepath)?;
        Ok(Configuration::new(environment_id, configuration)?)
    }

    /// Returns a mapping of segment ID to `Segment` for all segments referenced
    /// by the given `segment_rules`.
    fn get_segments_for_segment_rules(
        segments: &[Segment],
        segment_rules: &[SegmentRule],
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

    pub(crate) fn get_feature_ids_refs(&self) -> Vec<&String> {
        self.features.keys().collect()
    }

    pub(crate) fn get_property_ids_refs(&self) -> Vec<&String> {
        self.properties.keys().collect()
    }
}

impl ConfigurationProvider for Configuration {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        Ok(self.get_feature_ids_refs().into_iter().cloned().collect())
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        // Get the feature from the snapshot
        let (feature, segment_rules) = self
            .features
            .get(feature_id)
            .ok_or_else(|| ConfigurationDataError::FeatureNotFound(feature_id.to_string()))?;

        let enabled_value = (feature.r#type, feature.enabled_value.clone()).try_into()?;
        let disabled_value = (feature.r#type, feature.disabled_value.clone()).try_into()?;
        Ok(FeatureSnapshot::new(
            feature.enabled,
            enabled_value,
            disabled_value,
            feature.rollout_percentage,
            &feature.name,
            feature_id,
            segment_rules.clone(),
            None,
        ))
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        Ok(self.get_property_ids_refs().into_iter().cloned().collect())
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        // Get the property from the snapshot
        let (property, segment_rules) = self
            .properties
            .get(property_id)
            .ok_or_else(|| ConfigurationDataError::PropertyNotFound(property_id.to_string()))?;

        let value = (property.r#type, property.value.clone()).try_into()?;
        Ok(PropertySnapshot::new(
            value,
            segment_rules.clone(),
            &property.name,
            &property.property_id,
            None,
        ))
    }

    fn is_online(&self) -> Result<bool> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::models::tests::example_configuration_enterprise_path;
    use crate::models::ConfigurationJson;

    use rstest::*;

    #[rstest]
    fn test_filter_configurations(example_configuration_enterprise_path: PathBuf) {
        let content = std::fs::File::open(example_configuration_enterprise_path)
            .expect("file should open read only");
        let config_json: ConfigurationJson =
            serde_json::from_reader(content).expect("Error parsing JSON into Configuration");

        let result = Configuration::new("does_for_sure_not_exist", config_json);
        assert!(result.is_err());

        assert!(matches!(
                result.unwrap_err(),
                 ConfigurationDataError::EnvironmentNotFound(ref environment_id) if environment_id == "does_for_sure_not_exist"));
    }
}

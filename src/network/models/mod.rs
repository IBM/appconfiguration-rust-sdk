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

//! Models used to interchange information with servers. These models
//! are used for de/serialization purposes.

mod configuration;
mod metering;

pub(crate) use configuration::{
    ConfigValue, ConfigurationJson, Feature, Property, Rule, Segment, SegmentRule, ValueType,
};
pub(crate) use metering::{MeteringDataJson, MeteringDataUsageJson};

#[cfg(test)]
pub(crate) use configuration::Segments;

#[cfg(test)]
pub(crate) mod tests {

    use super::*;
    use rstest::*;
    use std::{fs, path::PathBuf};

    #[fixture]
    // Provides the path to the configuration data file
    pub(crate) fn example_configuration_enterprise_path() -> PathBuf {
        // Create a configuration object from the data files
        let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");
        mocked_data
    }

    #[fixture]
    // Creates a [`ConfigurationJson`] object from the data files
    pub(crate) fn example_configuration_enterprise(
        example_configuration_enterprise_path: PathBuf,
    ) -> ConfigurationJson {
        let content = fs::File::open(example_configuration_enterprise_path)
            .expect("file should open read only");
        let configuration: ConfigurationJson =
            serde_json::from_reader(content).expect("Error parsing JSON into Configuration");
        configuration
    }

    #[fixture]
    pub(crate) fn configuration_feature1_enabled() -> ConfigurationJson {
        ConfigurationJson {
            environments: vec![configuration::Environment {
                environment_id: "environment_id".to_string(),
                features: vec![Feature {
                    name: "F1".to_string(),
                    feature_id: "f1".to_string(),
                    r#type: ValueType::Numeric,
                    format: None,
                    enabled_value: ConfigValue(serde_json::Value::Number(42.into())),
                    disabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                    segment_rules: Vec::new(),
                    enabled: true,
                    rollout_percentage: 0,
                }],
                properties: Vec::new(),
            }],
            segments: Vec::new(),
        }
    }

    #[fixture]
    pub(crate) fn configuration_property1_enabled() -> ConfigurationJson {
        ConfigurationJson {
            environments: vec![configuration::Environment {
                environment_id: "environment_id".to_string(),
                properties: vec![Property {
                    name: "P1".to_string(),
                    property_id: "p1".to_string(),
                    r#type: ValueType::Numeric,
                    format: None,
                    value: ConfigValue(serde_json::Value::Number(42.into())),
                    segment_rules: Vec::new(),
                    tags: None,
                }],
                features: Vec::new(),
            }],
            segments: Vec::new(),
        }
    }

    #[fixture]
    pub(crate) fn configuration_unordered_segment_rules() -> ConfigurationJson {
        let segment_rules = vec![
            SegmentRule {
                rules: vec![configuration::Segments {
                    segments: vec!["some_segment_id_1".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-48).into())),
                order: 1,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            },
            SegmentRule {
                rules: vec![configuration::Segments {
                    segments: vec!["some_segment_id_2".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-49).into())),
                order: 0,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            },
        ];
        assert!(segment_rules[0].order > segment_rules[1].order);

        ConfigurationJson {
            environments: vec![configuration::Environment {
                environment_id: "environment_id".to_string(),
                features: vec![Feature {
                    name: "F1".to_string(),
                    feature_id: "f1".to_string(),
                    r#type: ValueType::Numeric,
                    format: None,
                    enabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                    disabled_value: ConfigValue(serde_json::Value::Number((2).into())),
                    segment_rules: segment_rules.clone(),
                    enabled: true,
                    rollout_percentage: 100,
                }],
                properties: vec![Property {
                    name: "P1".to_string(),
                    property_id: "f1".to_string(),
                    r#type: ValueType::Numeric,
                    format: None,
                    value: ConfigValue(serde_json::Value::Number((-42).into())),
                    segment_rules,
                    tags: None,
                }],
            }],
            segments: vec![
                Segment {
                    name: "".into(),
                    segment_id: "some_segment_id_1".into(),
                    description: "".into(),
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
                Segment {
                    name: "".into(),
                    segment_id: "some_segment_id_2".into(),
                    description: "".into(),
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            ],
        }
    }
}

// (C) Copyright IBM Corp. 2025.
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

use serde::Deserialize;

use super::Segment;
use crate::network::serialization::environment::Environment;
use crate::{errors::DeserializationError, Error, Result};

/// Represents AppConfig data in a structure intended for data exchange
/// (typically JSON encoded) used by
/// - AppConfig Server REST API (/config endpoint)
/// - AppConfig database dumps (via Web GUI)
/// - Offline configuration files used in offline-mode
#[derive(Debug, Deserialize)]
pub(crate) struct ConfigurationJson {
    pub environments: Vec<Environment>,
    pub segments: Vec<Segment>,
}

impl ConfigurationJson {
    /// Parses a ConfigurationJson from a file
    pub(crate) fn new(filepath: &std::path::Path) -> Result<Self> {
        let file = std::fs::File::open(filepath).map_err(|_| {
            Error::Other(format!(
                "File '{}' doesn't exist or cannot be read",
                filepath.display()
            ))
        })?;
        let reader = std::io::BufReader::new(file);

        serde_json::from_reader(reader).map_err(|e| {
            Error::DeserializationError(DeserializationError {
                string: format!(
                    "Error deserializing Configuration from file '{}'",
                    filepath.display()
                ),
                source: e.into(),
            })
        })
    }
}

#[cfg(test)]
pub(crate) mod fixtures {

    use crate::models::Configuration;
    use crate::network::serialization::config_value::ConfigValue;
    use crate::network::serialization::segments::Segments;
    use crate::network::serialization::{Feature, Property, Rule, SegmentRule, ValueType};

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
    ) -> Configuration {
        let content = fs::File::open(example_configuration_enterprise_path)
            .expect("file should open read only");
        let config_json: ConfigurationJson =
            serde_json::from_reader(content).expect("Error parsing JSON into Configuration");
        Configuration::new("dev", config_json).unwrap()
    }

    #[fixture]
    pub(crate) fn configuration_feature1_enabled() -> Configuration {
        let environment_id = "environment_id".to_string();
        let config_json = ConfigurationJson {
            environments: vec![Environment {
                environment_id: environment_id.clone(),
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
        };
        Configuration::new(&environment_id, config_json).unwrap()
    }

    #[fixture]
    pub(crate) fn configuration_property1_enabled() -> Configuration {
        let environment_id = "environment_id".to_string();
        let config_json = ConfigurationJson {
            environments: vec![Environment {
                environment_id: environment_id.clone(),
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
        };
        Configuration::new(&environment_id, config_json).unwrap()
    }

    #[fixture]
    pub(crate) fn configuration_unordered_segment_rules() -> Configuration {
        let segment_rules = vec![
            SegmentRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_1".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-48).into())),
                order: 1,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            },
            SegmentRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_2".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-49).into())),
                order: 0,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            },
        ];
        assert!(segment_rules[0].order > segment_rules[1].order);

        let environment_id = "environment_id".to_string();
        let config_json = ConfigurationJson {
            environments: vec![Environment {
                environment_id: environment_id.clone(),
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
        };
        Configuration::new(&environment_id, config_json).unwrap()
    }
}

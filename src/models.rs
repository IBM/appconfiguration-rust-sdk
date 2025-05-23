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

use std::fmt::Display;

use serde::Deserialize;

use crate::{errors::DeserializationError, Error, Result, Value};

/// Represents AppConfig data in a structure intended for data exchange
/// (typically JSON encoded) used by
/// - AppConfig Server REST API (/config endpoint)
/// - AppConfig database dumps (via Web GUI)
/// - Offline configuration files used in offline-mode
#[derive(Debug, Deserialize)]
pub struct ConfigurationJson {
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

#[derive(Debug, Deserialize)]
pub struct Environment {
    pub environment_id: String,
    pub features: Vec<Feature>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub segment_id: String,
    pub description: String,
    pub tags: Option<String>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Feature {
    pub name: String,
    pub feature_id: String,
    pub r#type: ValueType,
    pub format: Option<String>,
    pub enabled_value: ConfigValue,
    pub disabled_value: ConfigValue,
    // NOTE: why is this field called `segment_rules` and not `targeting_rules`?
    // This causes quite som ambiguity with SegmentRule vs TargetingRule.
    pub segment_rules: Vec<SegmentRule>,
    pub enabled: bool,
    pub rollout_percentage: u32,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub property_id: String,
    pub r#type: ValueType,
    pub tags: Option<String>,
    pub format: Option<String>,
    pub value: ConfigValue,
    // NOTE: why is this field called `segment_rules` and not `targeting_rules`?
    // This causes quite som ambiguity with SegmentRule vs TargetingRule.
    pub segment_rules: Vec<SegmentRule>,
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum ValueType {
    #[serde(rename(deserialize = "NUMERIC"))]
    Numeric,
    #[serde(rename(deserialize = "BOOLEAN"))]
    Boolean,
    #[serde(rename(deserialize = "STRING"))]
    String,
}

impl Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Numeric => "NUMERIC",
            Self::Boolean => "BOOLEAN",
            Self::String => "STRING",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ConfigValue(pub(crate) serde_json::Value);

impl ConfigValue {
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.0.as_u64()
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    pub fn as_boolean(&self) -> Option<bool> {
        self.0.as_bool()
    }

    pub fn as_string(&self) -> Option<String> {
        self.0.as_str().map(|s| s.to_string())
    }

    pub fn is_default(&self) -> bool {
        if let Some(s) = self.0.as_str() {
            s == "$default"
        } else {
            false
        }
    }
}

impl Display for ConfigValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<(ValueType, ConfigValue)> for Value {
    type Error = crate::Error;

    fn try_from(value: (ValueType, ConfigValue)) -> std::result::Result<Self, Self::Error> {
        let (kind, value) = value;
        match kind {
            ValueType::Numeric => {
                if let Some(n) = value.as_i64() {
                    Ok(Value::Int64(n))
                } else if let Some(n) = value.as_u64() {
                    Ok(Value::UInt64(n))
                } else if let Some(n) = value.as_f64() {
                    Ok(Value::Float64(n))
                } else {
                    Err(crate::Error::ProtocolError(
                        "Cannot convert numeric type".to_string(),
                    ))
                }
            }
            ValueType::Boolean => value
                .as_boolean()
                .map(Value::Boolean)
                .ok_or(crate::Error::MismatchType),
            ValueType::String => value
                .as_string()
                .map(Value::String)
                .ok_or(crate::Error::MismatchType),
        }
    }
}

/// Represents a Rule of a Segment.
/// Those are the rules to check if an entity belongs to a segment.
/// NOTE: This is easily confused with `TargetingRule`, which is
/// sometimes also called "SegmentRule".
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Rule {
    pub attribute_name: String,
    pub operator: String,
    pub values: Vec<String>,
}

/// Associates a Feature/Property to one or more Segments
/// NOTE: This is easily confused with `SegmentRule`, as the field name in
/// Features containing TargetingRules is called `segment_rules`
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct SegmentRule {
    /// The list of targeted segments
    /// NOTE: no rules by itself, but the rules are found in the segments
    /// NOTE: why list of lists?
    /// NOTE: why is this field called "rules"?
    pub rules: Vec<Segments>,
    pub value: ConfigValue,
    pub order: u32,
    pub rollout_percentage: Option<ConfigValue>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Segments {
    pub segments: Vec<String>,
}

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
            environments: vec![Environment {
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
            environments: vec![Environment {
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

        ConfigurationJson {
            environments: vec![Environment {
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

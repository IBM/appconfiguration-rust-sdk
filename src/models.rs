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

use crate::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct Configuration {
    pub environments: Vec<Environment>,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Environment {
    #[serde(rename = "name")]
    _name: String,
    pub environment_id: String,
    pub features: Vec<Feature>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Segment {
    #[serde(rename = "name")]
    pub _name: String,
    pub segment_id: String,
    #[serde(rename = "description")]
    pub _description: String,
    #[serde(rename = "tags")]
    pub _tags: Option<String>,
    pub rules: Vec<SegmentRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Feature {
    pub name: String,
    pub feature_id: String,
    #[serde(rename = "type")]
    pub kind: ValueKind,
    #[serde(rename = "format")]
    pub _format: Option<String>,
    pub enabled_value: ConfigValue,
    pub disabled_value: ConfigValue,
    pub segment_rules: Vec<TargetingRule>,
    pub enabled: bool,
    pub rollout_percentage: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Property {
    pub name: String,
    pub property_id: String,
    #[serde(rename = "type")]
    pub kind: ValueKind,
    #[serde(rename = "tags")]
    pub _tags: Option<String>,
    #[serde(rename = "format")]
    pub _format: Option<String>,
    pub value: ConfigValue,
    pub segment_rules: Vec<TargetingRule>,
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq)]
pub(crate) enum ValueKind {
    #[serde(rename(deserialize = "NUMERIC"))]
    Numeric,
    #[serde(rename(deserialize = "BOOLEAN"))]
    Boolean,
    #[serde(rename(deserialize = "STRING"))]
    String,
}

impl Display for ValueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Numeric => "NUMERIC",
            Self::Boolean => "BOOLEAN",
            Self::String => "STRING",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConfigValue(pub(crate) serde_json::Value);

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

impl TryFrom<(ValueKind, ConfigValue)> for Value {
    type Error = crate::Error;

    fn try_from(value: (ValueKind, ConfigValue)) -> Result<Self, Self::Error> {
        let (kind, value) = value;
        match kind {
            ValueKind::Numeric => {
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
            ValueKind::Boolean => value
                .as_boolean()
                .map(Value::Boolean)
                .ok_or(crate::Error::MismatchType),
            ValueKind::String => value
                .as_string()
                .map(Value::String)
                .ok_or(crate::Error::MismatchType),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SegmentRule {
    pub attribute_name: String,
    pub operator: String,
    pub values: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct TargetingRule {
    pub rules: Vec<Segments>,
    pub value: ConfigValue,
    pub order: u32,
    pub rollout_percentage: Option<ConfigValue>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Segments {
    pub segments: Vec<String>,
}

#[cfg(test)]
pub(crate) mod tests {

    use super::*;
    use rstest::*;
    use std::{fs, path::PathBuf};

    #[fixture]
    pub(crate) fn example_configuration_enterprise() -> Configuration {
        // Create a configuration object from the data files

        let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");

        let content = fs::File::open(mocked_data).expect("file should open read only");
        let configuration: Configuration =
            serde_json::from_reader(content).expect("Error parsing JSON into Configuration");
        configuration
    }

    #[fixture]
    pub(crate) fn configuration_feature1_enabled() -> Configuration {
        Configuration {
            environments: vec![Environment {
                _name: "name".to_string(),
                environment_id: "environment_id".to_string(),
                features: vec![Feature {
                    name: "F1".to_string(),
                    feature_id: "f1".to_string(),
                    kind: ValueKind::Numeric,
                    _format: None,
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
    pub(crate) fn configuration_property1_enabled() -> Configuration {
        Configuration {
            environments: vec![Environment {
                _name: "name".to_string(),
                environment_id: "environment_id".to_string(),
                properties: vec![Property {
                    name: "P1".to_string(),
                    property_id: "p1".to_string(),
                    kind: ValueKind::Numeric,
                    _format: None,
                    value: ConfigValue(serde_json::Value::Number(42.into())),
                    segment_rules: Vec::new(),
                    _tags: None,
                }],
                features: Vec::new(),
            }],
            segments: Vec::new(),
        }
    }
}

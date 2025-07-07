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

use crate::{errors::DeserializationError, Error, Result};
use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
pub(crate) struct Environment {
    pub environment_id: String,
    pub features: Vec<Feature>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Segment {
    pub name: String,
    pub segment_id: String,
    pub description: String,
    pub tags: Option<String>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Feature {
    pub name: String,
    pub feature_id: String,
    pub r#type: ValueType,
    pub format: Option<String>,
    pub enabled_value: ConfigValue,
    pub disabled_value: ConfigValue,
    pub segment_rules: Vec<SegmentRule>,
    pub enabled: bool,
    pub rollout_percentage: u32,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Property {
    pub name: String,
    pub property_id: String,
    pub r#type: ValueType,
    pub tags: Option<String>,
    pub format: Option<String>,
    pub value: ConfigValue,
    pub segment_rules: Vec<SegmentRule>,
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) enum ValueType {
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

/// Represents a Rule of a Segment.
/// Those are the rules to check if an entity belongs to a segment.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Rule {
    pub attribute_name: String,
    pub operator: String,
    pub values: Vec<String>,
}

/// Associates a Feature/Property to one or more Segments
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct SegmentRule {
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
pub(crate) struct Segments {
    pub segments: Vec<String>,
}

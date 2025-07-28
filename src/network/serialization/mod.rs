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

//! Models that are used for de/serialization of the data interchanged with
//! the server

mod config_value;
mod configuration;
mod environment;
mod feature;
mod property;
mod rule;
mod segment;
mod segment_rule;
mod segments;
mod value_type;

pub(crate) use configuration::ConfigurationJson;
pub(crate) use feature::Feature;
pub(crate) use property::Property;
pub(crate) use rule::Rule;
pub(crate) use segment::Segment;
pub(crate) use segment_rule::SegmentRule;
pub(crate) use value_type::ValueType;

use crate::Value;

impl TryFrom<(ValueType, config_value::ConfigValue)> for Value {
    type Error = crate::Error;

    fn try_from(
        value: (ValueType, config_value::ConfigValue),
    ) -> std::result::Result<Self, Self::Error> {
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

#[cfg(test)]
pub(crate) mod fixtures {
    pub(crate) use super::configuration::fixtures::*;
    pub(crate) use super::segment::fixtures::*;
    pub(crate) use super::segment_rule::fixtures::*;
}

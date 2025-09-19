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

use super::errors::CheckOperatorErrorDetail;
use crate::network::serialization::{Rule, Segment};
use crate::segment_evaluation::errors::SegmentEvaluationError;
use crate::{Entity, Value};

pub(crate) trait MatchesEntity {
    type Error;

    fn matches_entity(&self, entity: &impl Entity) -> std::result::Result<bool, Self::Error>;
}

impl MatchesEntity for Segment {
    type Error = SegmentEvaluationError;

    /// A [`Segment`] matches an [`Entity`] iif:
    /// * ALL the rules match the entity
    fn matches_entity(&self, entity: &impl Entity) -> std::result::Result<bool, Self::Error> {
        self.rules
            .iter()
            .map(|rule| {
                rule.matches_entity(entity)
                    .map_err(|(e, rule_value)| (e, self, rule, rule_value).into())
            })
            .collect::<std::result::Result<Vec<bool>, _>>()
            .map(|v| v.iter().all(|&x| x))
    }
}

impl MatchesEntity for Rule {
    type Error = (CheckOperatorErrorDetail, String);

    /// A [`Rule`] matches an [`Entity`] iif:
    /// * the entity contains the requested attribute, AND
    /// * the entity attribute satisfies ANY of the rule values.
    ///
    /// TODO: What if rules.values is empty? Now it returns false
    fn matches_entity(&self, entity: &impl Entity) -> std::result::Result<bool, Self::Error> {
        entity
            .get_attributes()
            .get(&self.attribute_name)
            .map_or(Ok(false), |attr_value| {
                self.values
                    .iter()
                    .map(|value| {
                        check_operator(attr_value, &self.operator, value)
                            .map_err(|e| (e, value.to_owned()))
                    })
                    .collect::<std::result::Result<Vec<bool>, _>>()
                    .map(|v| v.iter().any(|&x| x))
            })
    }
}

fn check_operator(
    attribute_value: &Value,
    operator: &str,
    reference_value: &str,
) -> std::result::Result<bool, CheckOperatorErrorDetail> {
    match operator {
        "is" => match attribute_value {
            Value::String(data) => Ok(*data == reference_value),
            Value::Boolean(data) => Ok(*data == reference_value.parse::<bool>()?),
            Value::Float64(data) => Ok(*data == reference_value.parse::<f64>()?),
            Value::UInt64(data) => Ok(*data == reference_value.parse::<u64>()?),
            Value::Int64(data) => Ok(*data == reference_value.parse::<i64>()?),
        },
        "contains" => match attribute_value {
            Value::String(data) => Ok(data.contains(reference_value)),
            _ => Err(CheckOperatorErrorDetail::StringExpected),
        },
        "startsWith" => match attribute_value {
            Value::String(data) => Ok(data.starts_with(reference_value)),
            _ => Err(CheckOperatorErrorDetail::StringExpected),
        },
        "endsWith" => match attribute_value {
            Value::String(data) => Ok(data.ends_with(reference_value)),
            _ => Err(CheckOperatorErrorDetail::StringExpected),
        },
        "greaterThan" => match attribute_value {
            // TODO: Go implementation also compares strings (by parsing them as floats). Do we need this?
            //       https://github.com/IBM/appconfiguration-go-sdk/blob/master/lib/internal/models/Rule.go#L82
            // TODO: we could have numbers not representable as f64, maybe we should try to parse it to i64 and u64 too?
            Value::Float64(data) => Ok(*data > reference_value.parse()?),
            Value::UInt64(data) => Ok(*data > reference_value.parse()?),
            Value::Int64(data) => Ok(*data > reference_value.parse()?),
            _ => Err(CheckOperatorErrorDetail::EntityAttrNotANumber),
        },
        "lesserThan" => match attribute_value {
            Value::Float64(data) => Ok(*data < reference_value.parse()?),
            Value::UInt64(data) => Ok(*data < reference_value.parse()?),
            Value::Int64(data) => Ok(*data < reference_value.parse()?),
            _ => Err(CheckOperatorErrorDetail::EntityAttrNotANumber),
        },
        "greaterThanEquals" => match attribute_value {
            Value::Float64(data) => Ok(*data >= reference_value.parse()?),
            Value::UInt64(data) => Ok(*data >= reference_value.parse()?),
            Value::Int64(data) => Ok(*data >= reference_value.parse()?),
            _ => Err(CheckOperatorErrorDetail::EntityAttrNotANumber),
        },
        "lesserThanEquals" => match attribute_value {
            Value::Float64(data) => Ok(*data <= reference_value.parse()?),
            Value::UInt64(data) => Ok(*data <= reference_value.parse()?),
            Value::Int64(data) => Ok(*data <= reference_value.parse()?),
            _ => Err(CheckOperatorErrorDetail::EntityAttrNotANumber),
        },
        _ => Err(CheckOperatorErrorDetail::OperatorNotImplemented),
    }
}

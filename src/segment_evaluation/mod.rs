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

pub(crate) mod errors;

use std::collections::HashMap;

use crate::entity::Entity;
use crate::errors::Error;
use crate::errors::Result;
use crate::models::Segment;
use crate::models::TargetingRule;
use crate::models::ValueKind;
use crate::Value;
use errors::{CheckOperatorErrorDetail, SegmentEvaluationError};

#[derive(Debug)]
pub(crate) struct SegmentRules {
    targeting_rules: Vec<TargetingRule>,
    segments: HashMap<String, Segment>,
    kind: ValueKind,
}

// TODO: We should rename this to TargetingRules
impl SegmentRules {
    pub(crate) fn new(
        segments: HashMap<String, Segment>,
        targeting_rules: Vec<TargetingRule>,
        kind: ValueKind,
    ) -> Self {
        Self {
            segments,
            targeting_rules,
            kind,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targeting_rules.is_empty()
    }

    /// Find the segment rule which matches for a given entity.
    /// Returns the matching SegmentRule and the Segment which the entity was
    /// associated to. (A SegmentRule/TargetingRule can point to multiple Segments)
    pub(crate) fn find_applicable_targeting_rule_and_segment_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<Option<(SegmentRule, &Segment)>> {
        for targeting_rule in self.targeting_rules.iter() {
            if let Some(segment) = find_segment_of_targeting_rule_which_applies_to_entity(&self.segments, targeting_rule, entity)? {
                return Ok(Some(
                    (
                    SegmentRule {
                        targeting_rule,
                        kind: self.kind,
                    },
                    segment
                    )
                ));
            }
        }
        Ok(None)
    }
}

// TODO: We should rename this to TargetingRule, it is basically encapsulation one.
// (need to avoid conflict with models::TargetingRule)
#[derive(Debug)]
pub(crate) struct SegmentRule<'a> {
    targeting_rule: &'a TargetingRule,
    kind: ValueKind,
}

impl SegmentRule<'_> {
    fn is_default(&self) -> bool {
        self.targeting_rule.value.is_default()
    }

    /// Returns the rollout percentage using the following logic:
    /// * If there is not rollout percentage in the [`TargetingRule`] it returns an error
    /// * If the rollout value in the [`TargetingRule`] is equal to "$default" it will return
    ///   the given `default` argument.
    /// * Otherwise it will return the rollout value from the [`TargetingRule`] converted to u32
    pub(crate) fn rollout_percentage(&self, default: u32) -> Result<u32> {
        self.targeting_rule
            .rollout_percentage
            .as_ref()
            .map(|v| {
                if v.is_default() {
                    Ok(default)
                } else {
                    let value: u64 = v.as_u64().ok_or(Error::ProtocolError(
                        "Rollout value is not u64.".to_string(),
                    ))?;

                    value.try_into().map_err(|e| {
                        Error::ProtocolError(format!(
                            "Invalid rollout value. Could not convert to u32: {e}"
                        ))
                    })
                }
            })
            .transpose()?
            .ok_or(Error::ProtocolError("Rollout is missing".to_string()))
    }

    /// Returns the value using the following logic:
    /// * If the value in the [`TargetingRule`] is equal to "$default" it will return
    ///   the given `default` argument.
    /// * Otherwise it will return the value from the [`TargetingRule`]
    pub(crate) fn value(&self, default: &Value) -> Result<Value> {
        if self.is_default() {
            Ok(default.clone())
        } else {
            (self.kind, self.targeting_rule.value.clone()).try_into()
        }
    }
}

// Finds out if a given TargetingRule (referring to multiple Segments) applies to a given entity.
// Basically this means it returns true, if one of the segments referred to by
// the targeting_rule matches the entity.
fn find_segment_of_targeting_rule_which_applies_to_entity<'a>(
    segments: &'a HashMap<String, Segment>,
    targeting_rule: &TargetingRule,
    entity: &impl Entity,
) -> std::result::Result<Option<&'a Segment>, SegmentEvaluationError> {
    // NOTE: In the JSON model the targeted segments (list of list) are called "rules" of a targeting rule.
    let targeted_segment_list_of_list = &targeting_rule.rules;
    for targeted_segment_list in targeted_segment_list_of_list.iter() {
        return Ok(find_segment_which_applies_to_entity(segments, &targeted_segment_list.segments, entity)?);
    }
    Ok(None)
}

fn find_segment_which_applies_to_entity<'a>(
    segments: &'a HashMap<String, Segment>,
    segment_ids: &[String],
    entity: &impl Entity,
) -> std::result::Result<Option<&'a Segment>, SegmentEvaluationError> {
    for segment_id in segment_ids.iter() {
        let segment = segments
            .get(segment_id)
            .ok_or(SegmentEvaluationError::SegmentIdNotFound(
                segment_id.clone(),
            ))?;
        let applies = belong_to_segment(segment, entity.get_attributes())?;
        if applies {
            return Ok(Some(segment));
        }
    }
    Ok(None)
}

fn belong_to_segment(
    segment: &Segment,
    attrs: HashMap<String, Value>,
) -> std::result::Result<bool, SegmentEvaluationError> {
    for rule in segment.rules.iter() {
        let operator = &rule.operator;
        let attr_name = &rule.attribute_name;
        let attr_value = attrs.get(attr_name);
        if attr_value.is_none() {
            return Ok(false);
        }
        let rule_result = match attr_value {
            None => {
                println!("Warning: Operation '{attr_name}' '{operator}' '[...]' failed to evaluate: '{attr_name}' not found in entity");
                false
            }
            Some(attr_value) => {
                // FIXME: the following algorithm is too hard to read. Is it just me or do we need to simplify this?
                // One of the values needs to match.
                // Find a candidate (a candidate corresponds to a value which matches or which might match but the operator failed):
                let candidate = rule
                    .values
                    .iter()
                    .find_map(|value| match check_operator(attr_value, operator, value) {
                        Ok(true) => Some(Ok::<_, SegmentEvaluationError>(())),
                        Ok(false) => None,
                        Err(e) => Some(Err((e, segment, rule, value).into())),
                    })
                    .transpose()?;
                // check if the candidate is good, or if the operator failed:
                candidate.is_some()
            }
        };
        // All rules must match:
        if !rule_result {
            return Ok(false);
        }
    }
    Ok(true)
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::errors::{EntityEvaluationError, Error};
    use crate::models::{ConfigValue, Segment, SegmentRule, Segments, TargetingRule};
    use rstest::*;

    #[fixture]
    fn segments() -> HashMap<String, Segment> {
        HashMap::from([
        (
            "some_segment_id_1".into(),
            Segment {
                _name: "".into(),
                segment_id: "some_segment_id_1".into(),
                _description: "".into(),
                _tags: None,
                rules: vec![SegmentRule {
                    attribute_name: "name".into(),
                    operator: "is".into(),
                    values: vec!["heinz".into()],
                }],
            },
        ),
        (
            "some_segment_id_2".into(),
            Segment {
                _name: "".into(),
                segment_id: "some_segment_id_2".into(),
                _description: "".into(),
                _tags: None,
                rules: vec![SegmentRule {
                    attribute_name: "name".into(),
                    operator: "is".into(),
                    values: vec!["peter".into()],
                }],
            },
        )
        ])
    }

    #[fixture]
    fn targeting_rules() -> Vec<TargetingRule> {
        vec![TargetingRule {
            rules: vec![Segments {
                segments: vec!["some_segment_id_1".into(),"some_segment_id_2".into()],
            }],
            value: ConfigValue(serde_json::Value::Number((-48).into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
        }]
    }

    #[rstest]
    fn test_targeting_rule_matches_and_correct_segment_reported_back(
        segments: HashMap<String, Segment>,
        targeting_rules: Vec<TargetingRule>,
    ) {
        let segment_rules = SegmentRules::new(segments, targeting_rules, ValueKind::String);
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("peter".to_string()))]),
        };
        let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
        // Segment evaluation should succeed:
        let (rule, segment) = rule.unwrap().unwrap();
        // And we should get the correct rule and the matched segment
        assert!(rule.targeting_rule.order == 0);
        assert!(segment.segment_id == "some_segment_id_2");
    }

    // SCENARIO - If the SDK user fail to pass the “attributes” for evaluation of featureflag which is segmented - we have considered that evaluation as “does not belong to any segment” and we serve the enabled_value.
    // EXAMPLE - Assume two teams are using same featureflag. One team is interested only in enabled_value & disabled_value. This team doesn’t pass attributes for  their evaluation. Other team wants to have overridden_value, as a result they update the featureflag by adding segment rules to it. This team passes attributes in their evaluation to get the overridden_value for matching segment, and enabled_value for non-matching segment.
    //  We should not fail the evaluation.
    #[rstest]
    fn test_attribute_not_found(
        segments: HashMap<String, Segment>,
        targeting_rules: Vec<TargetingRule>,
    ) {
        let segment_rules = SegmentRules::new(segments, targeting_rules, ValueKind::String);
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name2".into(), Value::from("heinz".to_string()))]),
        };
        let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
        // Segment evaluation should not fail:
        let rule = rule.unwrap();
        // But no segment should be found:
        assert!(rule.is_none())
    }

    // SCENARIO - The segment_id present in featureflag is invalid. In other words - the /config json dump has a featureflag, which has segment_rules. The segment_id in this segment_rules is invalid. Because this segment_id is not found in segments array.
    // This is a very good question. Firstly, the our server-side API are strongly validating inputs and give the responses. We have unittests & integration tests that verifies the input & output of /config API.  The response is always right. It is very much rare scenario where the API response has segment_id in featureflag object, that is not present is segments array.
    // We can agree to return error and mark evaluation as failed.
    #[rstest]
    fn test_invalid_segment_id(segments: HashMap<String, Segment>) {
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from(42.0))]),
        };
        let segment_rules = {
            let targeting_rules = vec![TargetingRule {
                rules: vec![Segments {
                    segments: vec!["non_existing_segment_id".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-48).into())),
                order: 0,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            }];
            SegmentRules::new(segments, targeting_rules, ValueKind::String)
        };
        let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
        // Error message should look something like this:
        //  Failed to evaluate entity: Failed to evaluate entity 'a2' against targeting rule '0'.
        //  Caused by: Segment 'non_existing_segment_id' not found.
        // We are checking here that the parts are present to allow debugging of config by the user:
        let e = rule.unwrap_err();
        assert!(matches!(e, Error::EntityEvaluationError(_)));
        let Error::EntityEvaluationError(EntityEvaluationError(
            SegmentEvaluationError::SegmentIdNotFound(ref segment_id),
        )) = e
        else {
            panic!("Error type mismatch!");
        };
        assert_eq!(segment_id, "non_existing_segment_id");
    }

    // SCENARIO - evaluating an operator fails. Meaning, [for example] user has added a numeric value(int/float) in appconfig segment attribute, but in their application they pass the attribute with a boolean value.
    // We can mark this as failure and return error.
    #[rstest]
    fn test_operator_failed(segments: HashMap<String, Segment>, targeting_rules: Vec<TargetingRule>) {
        let segment_rules = SegmentRules::new(segments, targeting_rules, ValueKind::String);
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from(42.0))]),
        };
        let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
        let e = rule.unwrap_err();
        assert!(matches!(e, Error::EntityEvaluationError(_)));
        let Error::EntityEvaluationError(EntityEvaluationError(
            SegmentEvaluationError::SegmentEvaluationFailed(ref error),
        )) = e
        else {
            panic!("Error type mismatch!");
        };
        assert_eq!(error.segment_id, "some_segment_id_1");
        assert_eq!(error.segment_rule_attribute_name, "name");
        assert_eq!(error.value, "heinz");
    }
}

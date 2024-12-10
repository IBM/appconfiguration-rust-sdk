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

use std::collections::HashMap;

use crate::errors::{Error, Result};
use crate::models::Segment;
use crate::{
    entity::{AttrValue, Entity},
    models::TargetingRule,
};

// For chaining errors creating useful error messages:
use anyhow::anyhow;
use anyhow::{Context, Result as AnyhowResult};

pub(crate) fn find_applicable_segment_rule_for_entity(
    segments: &HashMap<String, Segment>,
    segment_rules: impl Iterator<Item = TargetingRule>,
    entity: &impl Entity,
) -> Result<Option<TargetingRule>> {
    let mut targeting_rules = segment_rules.collect::<Vec<_>>();
    targeting_rules.sort_by(|a, b| a.order.cmp(&b.order));

    for targeting_rule in targeting_rules.into_iter() {
        if targeting_rule_applies_to_entity(segments, &targeting_rule, entity).map_err(|e| {
            // This terminates the use of anyhow in this module, converting all errors:
            let cause: String = e.chain().map(|c| format!("\nCaused by: {c}")).collect();
            Error::EntityEvaluationError(format!(
                "Failed to evaluate entity '{}' against targeting rule '{}'.{cause}",
                entity.get_id(),
                targeting_rule.order
            ))
        })? {
            return Ok(Some(targeting_rule));
        }
    }
    return Ok(None);
}

fn targeting_rule_applies_to_entity(
    segments: &HashMap<String, Segment>,
    targeting_rule: &TargetingRule,
    entity: &impl Entity,
) -> AnyhowResult<bool> {
    // TODO: we need to get the naming correct here to distinguish between rules, segments, segment_ids, targeting_rules etc. correctly
    let rules = &targeting_rule.rules;
    for rule in rules.iter() {
        let rule_applies = segment_applies_to_entity(segments, &rule.segments, entity)?;
        if rule_applies {
            return Ok(true);
        }
    }
    Ok(false)
}

fn segment_applies_to_entity(
    segments: &HashMap<String, Segment>,
    segment_ids: &[String],
    entity: &impl Entity,
) -> AnyhowResult<bool> {
    for segment_id in segment_ids.iter() {
        let segment = segments.get(segment_id).ok_or(Error::Other(
            format!("Segment '{segment_id}' not found.").into(),
        ))?;
        let applies = belong_to_segment(segment, entity.get_attributes())
            .context(format!("Failed to evaluate segment '{segment_id}'"))?;
        if applies {
            return Ok(true);
        }
    }
    Ok(false)
}

fn belong_to_segment(segment: &Segment, attrs: HashMap<String, AttrValue>) -> AnyhowResult<bool> {
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
                Ok(false)
            }
            Some(attr_value) => {
                // FIXME: the following algorithm is too hard to read. Is it just me or do we need to simplify this?
                // One of the values needs to match.
                // Find a candidate (a candidate corresponds to a value which matches or which might match but the operator failed):
                let candidate = rule.values.iter().find_map(|value| {
                    let result_for_value =
                        check_operator(attr_value, operator, value).context(format!(
                            "Operation '{attr_name}' '{operator}' '{value}' failed to evaluate."
                        ));
                    match result_for_value {
                        Ok(true) => Some(Ok(())),
                        Ok(false) => None,
                        Err(e) => Some(Err(e)),
                    }
                });
                // check if the candidate is good, or if the operator failed:
                match candidate {
                    None => Ok(false),
                    Some(Ok(())) => Ok(true),
                    Some(Err(e)) => Err(e),
                }
            }
        }?;
        // All rules must match:
        if !rule_result {
            return Ok(false);
        }
    }
    Ok(true)
}

fn check_operator(
    attribute_value: &AttrValue,
    operator: &str,
    reference_value: &str,
) -> AnyhowResult<bool> {
    match operator {
        "is" => match attribute_value {
            AttrValue::String(data) => Ok(*data == reference_value),
            AttrValue::Boolean(data) => {
                let result = *data
                    == reference_value
                        .parse::<bool>()
                        .map_err(|_| anyhow!("Entity attribute has unexpected type: Boolean."))?;
                Ok(result)
            }
            AttrValue::Numeric(data) => {
                let result = *data
                    == reference_value
                        .parse::<f64>()
                        .map_err(|_| anyhow!("Entity attribute has unexpected type: Number."))?;
                Ok(result)
            }
        },
        "contains" => match attribute_value {
            AttrValue::String(data) => Ok(data.contains(reference_value)),
            _ => Err(anyhow!("Entity attribute is not a string.")),
        },
        "startsWith" => match attribute_value {
            AttrValue::String(data) => Ok(data.starts_with(reference_value)),
            _ => Err(anyhow!("Entity attribute is not a string.")),
        },
        "endsWith" => match attribute_value {
            AttrValue::String(data) => Ok(data.ends_with(reference_value)),
            _ => Err(anyhow!("Entity attribute is not a string.")),
        },
        "greaterThan" => match attribute_value {
            // TODO: Go implementation also compares strings (by parsing them as floats). Do we need this?
            //       https://github.com/IBM/appconfiguration-go-sdk/blob/master/lib/internal/models/Rule.go#L82
            // TODO: we could have numbers not representable as f64, maybe we should try to parse it to i64 and u64 too?
            // TODO: we should have a different nesting style here: match the reference_value first and error out when given
            //       entity attr does not match. This would yield more natural error messages
            AttrValue::Numeric(data) => {
                let result = *data
                    > reference_value
                        .parse()
                        .map_err(|_| Error::Other("Value cannot convert into f64.".into()))?;
                Ok(result)
            }
            _ => Err(anyhow!("Entity attribute is not a number.")),
        },
        "lesserThan" => match attribute_value {
            AttrValue::Numeric(data) => {
                let result = *data
                    < reference_value
                        .parse()
                        .map_err(|_| Error::Other("Value cannot convert into f64.".into()))?;
                Ok(result)
            }
            _ => Err(anyhow!("Entity attribute is not a number.")),
        },
        "greaterThanEquals" => match attribute_value {
            AttrValue::Numeric(data) => {
                let result = *data
                    >= reference_value
                        .parse()
                        .map_err(|_| Error::Other("Value cannot convert into f64.".into()))?;
                Ok(result)
            }
            _ => Err(anyhow!("Entity attribute is not a number.")),
        },
        "lesserThanEquals" => match attribute_value {
            AttrValue::Numeric(data) => {
                let result = *data
                    <= reference_value
                        .parse()
                        .map_err(|_| Error::Other("Value cannot convert into f64.".into()))?;
                Ok(result)
            }
            _ => Err(anyhow!("Entity attribute is not a number.")),
        },
        _ => Err(anyhow!("Operator not implemented")),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        models::{ConfigValue, Segment, SegmentRule, Segments, TargetingRule},
        AttrValue,
    };
    use rstest::*;

    #[fixture]
    fn segments() -> HashMap<String, Segment> {
        HashMap::from([(
            "some_segment_id_1".into(),
            Segment {
                name: "".into(),
                segment_id: "".into(),
                description: "".into(),
                tags: None,
                rules: vec![SegmentRule {
                    attribute_name: "name".into(),
                    operator: "is".into(),
                    values: vec!["heinz".into()],
                }],
            },
        )])
    }

    #[fixture]
    fn segment_rules() -> Vec<TargetingRule> {
        vec![TargetingRule {
            rules: vec![Segments {
                segments: vec!["some_segment_id_1".into()],
            }],
            value: ConfigValue(serde_json::Value::Number((-48).into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
        }]
    }

    // SCENARIO - If the SDK user fail to pass the “attributes” for evaluation of featureflag which is segmented - we have considered that evaluation as “does not belong to any segment” and we serve the enabled_value.
    // EXAMPLE - Assume two teams are using same featureflag. One team is interested only in enabled_value & disabled_value. This team doesn’t pass attributes for  their evaluation. Other team wants to have overridden_value, as a result they update the featureflag by adding segment rules to it. This team passes attributes in their evaluation to get the overridden_value for matching segment, and enabled_value for non-matching segment.
    //  We should not fail the evaluation.
    #[rstest]
    fn test_attribute_not_found(
        segments: HashMap<String, Segment>,
        segment_rules: Vec<TargetingRule>,
    ) {
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name2".into(), AttrValue::from("heinz".to_string()))]),
        };
        let rule =
            find_applicable_segment_rule_for_entity(&segments, segment_rules.into_iter(), &entity);
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
            attributes: HashMap::from([("name".into(), AttrValue::from(42.0))]),
        };
        let segment_rules = vec![TargetingRule {
            rules: vec![Segments {
                segments: vec!["non_existing_segment_id".into()],
            }],
            value: ConfigValue(serde_json::Value::Number((-48).into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
        }];
        let rule =
            find_applicable_segment_rule_for_entity(&segments, segment_rules.into_iter(), &entity);
        // Error message should look something like this:
        //  Failed to evaluate entity: Failed to evaluate entity 'a2' against targeting rule '0'.
        //  Caused by: Segment 'non_existing_segment_id' not found.
        // We are checking here that the parts are present to allow debugging of config by the user:
        let msg = rule.unwrap_err().to_string();
        assert!(msg.contains("'a2'"));
        assert!(msg.contains("'0'"));
        assert!(msg.contains("'non_existing_segment_id'"));
        assert!(msg.contains("not found"));
    }

    // SCENARIO - evaluating an operator fails. Meaning, [for example] user has added a numeric value(int/float) in appconfig segment attribute, but in their application they pass the attribute with a boolean value.
    // We can mark this as failure and return error.
    #[rstest]
    fn test_operator_failed(segments: HashMap<String, Segment>, segment_rules: Vec<TargetingRule>) {
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), AttrValue::from(42.0))]),
        };
        let rule =
            find_applicable_segment_rule_for_entity(&segments, segment_rules.into_iter(), &entity);
        // Error message should look something like this:
        //  Failed to evaluate entity: Failed to evaluate entity 'a2' against targeting rule '0'.
        //  Caused by: Failed to evaluate segment 'some_segment_id_1'
        //  Caused by: Operation 'name' 'is' 'heinz' failed to evaluate.
        //  Caused by: Entity attribute has unexpected type: Number.
        // We are checking here that the parts are present to allow debugging of config by the user:
        let msg = rule.unwrap_err().to_string();
        assert!(msg.contains("'a2'"));
        assert!(msg.contains("'0'"));
        assert!(msg.contains("'some_segment_id_1'"));
        assert!(msg.contains("'name' 'is' 'heinz'"));
        assert!(msg.contains("Entity attribute has unexpected type: Number"));
    }
}

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
mod matches_attributes;
mod rule_operator;

use crate::Value;
use crate::entity::Entity;
use crate::errors::Error;
use crate::errors::Result;
use crate::models::{DELIMITER, ROLLOUT_TYPE_PROGRESSIVE};
use crate::network::serialization::{Segment, SegmentRule, ValueType};
use crate::segment_evaluation::matches_attributes::MatchesAttributes;
use crate::utils::{get_current_rollout_percentage, parse_rollout_configuration_phases};
use chrono::Utc;
use errors::SegmentEvaluationError;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetingRules {
    segment_rules: Vec<SegmentRule>,
    segments: HashMap<String, Segment>,
    r#type: ValueType,
    /// Map of segment rule rollout configurations (key: feature_id + DELIMITER + rule_id)
    rollout_config_map: HashMap<String, BTreeMap<i64, u32>>,
    /// Feature ID for this targeting rules (needed for progressive rollout)
    feature_id: String,
}

impl TargetingRules {
    pub(crate) fn new(
        segments: HashMap<String, Segment>,
        segment_rules: Vec<SegmentRule>,
        r#type: ValueType,
        feature_id: Option<&str>,
    ) -> Self {
        let mut rollout_config_map = HashMap::new();

        if let Some(feature_id) = feature_id {
            for segment_rule in &segment_rules {
                if segment_rule.rollout_type.as_deref() != Some(ROLLOUT_TYPE_PROGRESSIVE) {
                    continue;
                }

                if let Some(rollout_config) = &segment_rule.rollout_configuration {
                    if let Some(rule_id) = &segment_rule.rule_id {
                        let key = format!("{}{}{}", feature_id, DELIMITER, rule_id);
                        if let Ok(btree) = parse_rollout_configuration_phases(rollout_config) {
                            rollout_config_map.insert(key, btree);
                        }
                    }
                }
            }
        }

        Self {
            segments,
            segment_rules,
            r#type,
            rollout_config_map,
            feature_id: feature_id.unwrap_or_default().to_string(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segment_rules.is_empty()
    }

    /// Finds the [`TargetingRule`] and the [`Segment`] which a given entity can be associated to.
    /// Note: A feature/property can have multiple TargetingRules, which define a specific feature/property value. One TargetingRule can point to multiple Segments. Rules and Segments are iterated in order and the first match is reported.
    /// TODO: A TargetingRule can have Rules and Segments also have Rules. Those are easily confused. Especially, as TargetingRules are sometimes referred to as SegmentRules, which causes even greater confusion.
    pub(crate) fn find_applicable_targeting_rule_and_segment_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<Option<(TargetingRule<'_>, &Segment)>> {
        for segment_rule in self.segment_rules.iter() {
            if let Some(segment) = find_segment_of_targeting_rule_which_applies_to_entity(
                &self.segments,
                segment_rule,
                entity,
            )? {
                return Ok(Some((
                    TargetingRule {
                        segment_rule,
                        r#type: self.r#type,
                        rollout_config_map: &self.rollout_config_map,
                        feature_id: self.feature_id.clone(),
                    },
                    segment,
                )));
            }
        }
        Ok(None)
    }
}

#[derive(Debug)]
pub(crate) struct TargetingRule<'a> {
    segment_rule: &'a SegmentRule,
    r#type: ValueType,
    rollout_config_map: &'a HashMap<String, BTreeMap<i64, u32>>,
    feature_id: String,
}

impl TargetingRule<'_> {
    fn is_default(&self) -> bool {
        self.segment_rule.value.is_default()
    }

    pub(crate) fn order(&self) -> u32 {
        self.segment_rule.order
    }

    pub(crate) fn entity_id_for_hash(&self, entity_id: String) -> String {
        if self.segment_rule.rollout_type.as_deref() == Some(ROLLOUT_TYPE_PROGRESSIVE) {
            if let Some(rollout_config) = &self.segment_rule.rollout_configuration {
                // Append start_at for entity-stability in progressive rollout
                return format!("{}{}", entity_id, rollout_config.start_at);
            }
        }
        entity_id
    }

    /// Returns the rollout percentage using the following logic:
    /// * If there is not rollout percentage in the [`TargetingRule`] it returns an error
    /// * If the rollout value in the [`TargetingRule`] is equal to "$default" it will return
    ///   the given `default` argument.
    /// * Otherwise it will return the rollout value from the [`TargetingRule`] converted to u32
    pub(crate) fn rollout_percentage(&self, default: u32) -> Result<u32> {
        if self.segment_rule.rollout_type.as_deref() == Some(ROLLOUT_TYPE_PROGRESSIVE) {
            if let Some(rule_id) = &self.segment_rule.rule_id {
                let key = format!("{}{}{}", self.feature_id, DELIMITER, rule_id);
                if let Some(btree) = self.rollout_config_map.get(&key) {
                    let current_time_ms = Utc::now().timestamp_millis();
                    return Ok(get_current_rollout_percentage(btree, current_time_ms));
                }
            }
        }

        self.segment_rule
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
            (self.r#type, self.segment_rule.value.clone()).try_into()
        }
    }
}

// Finds out if a given TargetingRule (referring to multiple Segments) applies to a given entity.
// Basically this means it returns true, if one of the segments referred to by
// the targeting_rule matches the entity.
fn find_segment_of_targeting_rule_which_applies_to_entity<'a>(
    segments: &'a HashMap<String, Segment>,
    segment_rule: &SegmentRule,
    entity: &impl Entity,
) -> std::result::Result<Option<&'a Segment>, SegmentEvaluationError> {
    // NOTE: In the JSON model the targeted segments (list of list) are called "rules" of a targeting rule.
    let targeted_segment_list_of_list = &segment_rule.rules;
    for targeted_segment_list in targeted_segment_list_of_list.iter() {
        if let Some(segment) =
            find_segment_which_applies_to_entity(segments, &targeted_segment_list.segments, entity)?
        {
            return Ok(Some(segment));
        }
    }

    Ok(None)
}

fn find_segment_which_applies_to_entity<'a>(
    segments: &'a HashMap<String, Segment>,
    segment_ids: &[String],
    entity: &impl Entity,
) -> std::result::Result<Option<&'a Segment>, SegmentEvaluationError> {
    Ok(segment_ids
        .iter()
        .map(|segment_id| match segments.get(segment_id) {
            Some(segment) => segment
                .matches_attributes(&entity.get_attributes())
                .map(|v| v.then_some(segment)),
            None => Err(SegmentEvaluationError::SegmentIdNotFound(
                segment_id.clone(),
            )),
        })
        .collect::<std::result::Result<Vec<Option<&Segment>>, _>>()?
        .into_iter()
        .find(|s| s.is_some())
        .and_then(|v| v))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::errors::{EntityEvaluationError, Error};
    use crate::network::serialization::fixtures::{
        segment_rules_with_invalid_segment_id, some_segment_rules, some_segments,
    };
    use crate::network::serialization::{Segment, SegmentRule};
    use rstest::*;

    #[rstest]
    fn test_targeting_rule_matches_and_correct_segment_reported_back(
        some_segments: HashMap<String, Segment>,
        some_segment_rules: Vec<SegmentRule>,
    ) {
        let segment_rules =
            TargetingRules::new(some_segments, some_segment_rules, ValueType::String, None);
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("peter".to_string()))]),
        };

        {
            let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
            // Segment evaluation should succeed:
            let (rule, segment) = rule.unwrap().unwrap();
            // And we should get the correct rule and the matched segment
            assert!(rule.segment_rule.order == 0);
            assert!(segment.segment_id == "some_segment_id_2");
        }

        let entity = crate::tests::GenericEntity {
            id: "a3".into(),
            attributes: HashMap::from([("name".into(), Value::from("jane".to_string()))]),
        };
        {
            let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
            // Segment evaluation should succeed:
            let (rule, segment) = rule.unwrap().unwrap();
            // And we should get the correct rule and the matched segment
            assert!(rule.segment_rule.order == 0);
            assert!(segment.segment_id == "some_segment_id_3");
        }

        let entity = crate::tests::GenericEntity {
            id: "a3".into(),
            attributes: HashMap::from([("name".into(), Value::from("noname".to_string()))]),
        };
        {
            let rule = segment_rules.find_applicable_targeting_rule_and_segment_for_entity(&entity);
            // Segment evaluation should succeed, but no rule is found:
            assert!(rule.unwrap().is_none());
        }
    }

    // SCENARIO - If the SDK user fail to pass the “attributes” for evaluation of featureflag which is segmented - we have considered that evaluation as “does not belong to any segment” and we serve the enabled_value.
    // EXAMPLE - Assume two teams are using same featureflag. One team is interested only in enabled_value & disabled_value. This team doesn’t pass attributes for  their evaluation. Other team wants to have overridden_value, as a result they update the featureflag by adding segment rules to it. This team passes attributes in their evaluation to get the overridden_value for matching segment, and enabled_value for non-matching segment.
    //  We should not fail the evaluation.
    #[rstest]
    fn test_attribute_not_found(
        some_segments: HashMap<String, Segment>,
        some_segment_rules: Vec<SegmentRule>,
    ) {
        let segment_rules =
            TargetingRules::new(some_segments, some_segment_rules, ValueType::String, None);
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
    fn test_invalid_segment_id(
        some_segments: HashMap<String, Segment>,
        segment_rules_with_invalid_segment_id: Vec<SegmentRule>,
    ) {
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from(42.0))]),
        };
        let segment_rules = TargetingRules::new(
            some_segments,
            segment_rules_with_invalid_segment_id,
            ValueType::String,
            None,
        );
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
    fn test_operator_failed(
        some_segments: HashMap<String, Segment>,
        some_segment_rules: Vec<SegmentRule>,
    ) {
        let segment_rules =
            TargetingRules::new(some_segments, some_segment_rules, ValueType::String, None);
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

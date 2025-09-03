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

use crate::entity::Entity;
use crate::metering::{MeteringRecorderSender, MeteringSubject};
use crate::value::Value;
use crate::Feature;

use crate::segment_evaluation::TargetingRules;

use crate::errors::Result;

use std::io::Cursor;

use murmur3::murmur3_32;

/// Provides a snapshot of a [`Feature`].
#[derive(Debug)]
pub struct FeatureSnapshot {
    enabled: bool,
    enabled_value: Value,
    disabled_value: Value,
    rollout_percentage: u32,
    pub(crate) name: String,
    pub(crate) feature_id: String,
    segment_rules: TargetingRules,
    pub(crate) metering: Option<MeteringRecorderSender>,
}

impl FeatureSnapshot {
    pub(crate) fn new(
        enabled: bool,
        enabled_value: Value,
        disabled_value: Value,
        rollout_percentage: u32,
        name: &str,
        feature_id: &str,
        segment_rules: TargetingRules,
        metering: Option<MeteringRecorderSender>,
    ) -> Self {
        Self {
            enabled,
            enabled_value,
            disabled_value,
            rollout_percentage,
            name: name.to_string(),
            feature_id: feature_id.to_string(),
            segment_rules,
            metering,
        }
    }

    fn evaluate_feature_for_entity(&self, entity: &impl Entity) -> Result<Value> {
        if !self.enabled {
            self.record_evaluation(entity, None);
            return Ok(self.disabled_value.clone());
        }

        let (segment_rule, segment) = {
            if self.segment_rules.is_empty() || entity.get_attributes().is_empty() {
                // TODO: this makes only sense if there can be a rule which matches
                //       even on empty attributes
                // No match possible. Do not consider segment rules:
                (None, None)
            } else {
                self.segment_rules
                    .find_applicable_targeting_rule_and_segment_for_entity(entity)?
                    .unzip()
            }
        };

        self.record_evaluation(entity, segment);

        match segment_rule {
            Some(segment_rule) => {
                // Get rollout percentage
                let rollout_percentage =
                    segment_rule.rollout_percentage(self.rollout_percentage)?;

                // Should rollout?
                if Self::should_rollout(rollout_percentage, entity, &self.feature_id) {
                    segment_rule.value(&self.enabled_value)
                } else {
                    Ok(self.disabled_value.clone())
                }
            }
            None => self.use_rollout_percentage_to_get_value_from_feature_directly(entity),
        }
    }

    fn normalized_hash(data: &str) -> u32 {
        let hash = murmur3_32(&mut Cursor::new(data), 0).expect("Cannot hash the value.");
        (f64::from(hash) / f64::from(u32::MAX) * 100.0) as u32
    }

    fn should_rollout(rollout_percentage: u32, entity: &impl Entity, feature_id: &str) -> bool {
        let tag = format!("{}:{}", entity.get_id(), feature_id);
        rollout_percentage == 100 || Self::normalized_hash(&tag) < rollout_percentage
    }

    fn use_rollout_percentage_to_get_value_from_feature_directly(
        &self,
        entity: &impl Entity,
    ) -> Result<Value> {
        let rollout_percentage = self.rollout_percentage;
        if Self::should_rollout(rollout_percentage, entity, &self.feature_id) {
            Ok(self.enabled_value.clone())
        } else {
            Ok(self.disabled_value.clone())
        }
    }
}

impl Feature for FeatureSnapshot {
    fn get_name(&self) -> Result<String> {
        Ok(self.name.clone())
    }

    fn is_enabled(&self) -> Result<bool> {
        Ok(self.enabled)
    }

    fn get_value(&self, entity: &impl Entity) -> Result<Value> {
        self.evaluate_feature_for_entity(entity)
    }

    fn get_value_into<T: TryFrom<Value, Error = crate::Error>>(
        &self,
        entity: &impl Entity,
    ) -> Result<T> {
        let value = self.get_value(entity)?;
        value.try_into()
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use crate::network::serialization::fixtures::{create_one_segment_rule, one_segment_rule};
    use crate::network::serialization::{Rule, Segment, SegmentRule, ValueType};
    use rstest::rstest;
    use std::collections::HashMap;

    #[rstest]
    #[case("a1", false)]
    #[case("a2", true)]
    fn test_should_rollout(#[case] entity_id: &str, #[case] partial_rollout_expectation: bool) {
        let entity = crate::tests::GenericEntity {
            id: entity_id.into(),
            attributes: HashMap::new(),
        };
        let result = FeatureSnapshot::should_rollout(100, &entity, "f1");
        assert!(result);

        let result = FeatureSnapshot::should_rollout(0, &entity, "f1");
        assert!(!result);

        let result = FeatureSnapshot::should_rollout(50, &entity, "f1");
        assert_eq!(result, partial_rollout_expectation);

        let result = FeatureSnapshot::should_rollout(50, &entity, "f4");
        // We chose feature ID here so that we rollout exactly inverted to "f1"
        assert_eq!(result, !partial_rollout_expectation);
    }

    // Scenarios in which no segment rule matching should be performed.
    // So we expect to always return feature's enabled/disabled values depending on rollout percentage.
    #[rstest]
    // no attrs, no segment rules
    #[case([].into(), [].into())]
    // attrs but no segment rules
    #[case([].into(), [("key".into(), Value::from("value".to_string()))].into())]
    // no attrs but segment rules
    #[case(crate::network::serialization::fixtures::one_segment_rule(), [].into())]
    fn test_get_value_no_match_50_50_rollout(
        #[case] segment_rules: Vec<SegmentRule>,
        #[case] entity_attributes: HashMap<String, Value>,
    ) {
        let feature = {
            let segment_rules =
                TargetingRules::new(HashMap::new(), segment_rules, ValueType::Numeric);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                50,
                "F1",
                "f1",
                segment_rules,
                None,
            )
        };

        // One entity and feature combination which leads to no rollout:
        let entity = crate::tests::GenericEntity {
            id: "a1".into(),
            attributes: entity_attributes.clone(),
        };
        assert_eq!(
            FeatureSnapshot::normalized_hash(
                format!("{}:{}", entity.id, feature.feature_id).as_str()
            ),
            68
        );
        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &2));

        // One entity and feature combination which leads to rollout:
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: entity_attributes,
        };
        assert_eq!(
            FeatureSnapshot::normalized_hash(
                format!("{}:{}", entity.id, feature.feature_id).as_str()
            ),
            29
        );
        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }

    // If the feature is disabled, always the disabled value should be returned.
    #[test]
    fn test_get_value_disabled_feature() {
        let feature = {
            let segment_rules = TargetingRules::new(HashMap::new(), Vec::new(), ValueType::Numeric);
            FeatureSnapshot::new(
                false,
                Value::Int64(-42),
                Value::Int64(2),
                100,
                "F1",
                "f1",
                segment_rules,
                None,
            )
        };

        let entity = crate::entity::tests::TrivialEntity {};
        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &2));
    }

    // Get a feature value using different entities, matching or not matching a segment rule.
    // Uses rollout percentage to also test no rollout even if matched
    #[rstest]
    fn test_get_value_matching_a_rule(one_segment_rule: Vec<SegmentRule>) {
        let feature = {
            let segments = HashMap::from([(
                "some_segment_id".into(),
                Segment {
                    name: "".into(),
                    segment_id: "".into(),
                    description: None,
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = TargetingRules::new(segments, one_segment_rule, ValueType::Numeric);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                50,
                "F1",
                "f1",
                segment_rules,
                None,
            )
        };

        // matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-48)));

        // matching the segment + rollout disallowed
        let entity = crate::tests::GenericEntity {
            id: "a1".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &2));

        // not matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinzz".to_string()))]),
        };

        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }

    // The matched segment rule's value has a "$default" value.
    // In this case, the feature's enabled value should be used whenever the rule matches.
    #[test]
    fn test_get_value_matching_yielding_default_value() {
        let feature = {
            let segments = HashMap::from([(
                "some_segment_id".into(),
                Segment {
                    name: "".into(),
                    segment_id: "".into(),
                    description: None,
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = create_one_segment_rule(
                "some_segment_id".into(),
                serde_json::Value::String("$default".into()),
                serde_json::Value::Number((50).into()),
            );
            let segment_rules = TargetingRules::new(segments, segment_rules, ValueType::Numeric);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                50,
                "F1",
                "f1",
                segment_rules,
                None,
            )
        };

        // matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }

    // The matched segment rule's rollout percentage has a "$default" value.
    // In this case, the feature's rollout percentage should be used whenever the rule matches.
    #[test]
    fn test_get_value_matching_segment_rollout_default_value() {
        let feature = {
            let segments = HashMap::from([(
                "some_segment_id".into(),
                Segment {
                    name: "".into(),
                    segment_id: "".into(),
                    description: None,
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = create_one_segment_rule(
                "some_segment_id".into(),
                serde_json::Value::Number(48.into()),
                serde_json::Value::String("$default".into()),
            );
            let segment_rules = TargetingRules::new(segments, segment_rules, ValueType::Numeric);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                0,
                "F1",
                "f1",
                segment_rules,
                None,
            )
        };

        // matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &2));
    }

    /// This test ensures that the rust client is using the same hashing algorithm as to other clients.
    /// See same test for Node client:
    /// https://github.com/IBM/appconfiguration-node-sdk/blob/master/test/unit/configurations/internal/utils.test.js#L25
    #[test]
    fn test_normalized_hash() {
        assert_eq!(FeatureSnapshot::normalized_hash("entityId:featureId"), 41)
    }
}

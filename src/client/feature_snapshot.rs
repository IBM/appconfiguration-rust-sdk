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
use crate::value::Value;
use crate::Feature;

use super::feature_proxy::random_value;
use crate::segment_evaluation::SegmentRules;

use crate::errors::Result;

/// Provides a snapshot of a [`Feature`].
#[derive(Debug)]
pub struct FeatureSnapshot {
    enabled: bool,
    enabled_value: Value,
    disabled_value: Value,
    rollout_percentage: u32,
    name: String,
    feature_id: String,
    segment_rules: SegmentRules,
}

impl FeatureSnapshot {
    pub(crate) fn new(
        enabled: bool,
        enabled_value: Value,
        disabled_value: Value,
        rollout_percentage: u32,
        name: &str,
        feature_id: &str,
        segment_rules: SegmentRules,
    ) -> Self {
        Self {
            enabled,
            enabled_value,
            disabled_value,
            rollout_percentage,
            name: name.to_string(),
            feature_id: feature_id.to_string(),
            segment_rules,
        }
    }

    fn evaluate_feature_for_entity(&self, entity: &impl Entity) -> Result<Value> {
        if !self.enabled {
            return Ok(self.disabled_value.clone());
        }

        if self.segment_rules.is_empty() || entity.get_attributes().is_empty() {
            // No match possible. Do not consider segment rules:
            return self.use_rollout_percentage_to_get_value_from_feature_directly(entity);
        }

        match self
            .segment_rules
            .find_applicable_segment_rule_for_entity(entity)?
        {
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

    fn should_rollout(rollout_percentage: u32, entity: &impl Entity, feature_id: &str) -> bool {
        let tag = format!("{}:{}", entity.get_id(), feature_id);
        rollout_percentage == 100 || random_value(&tag) < rollout_percentage
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
    use crate::models::{ConfigValue, Segment, SegmentRule, Segments, TargetingRule, ValueKind};
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
    #[case([TargetingRule{rules: Vec::new(), value: ConfigValue(serde_json::json!("")), order: 0, rollout_percentage: None}].into(), [].into())]
    fn test_get_value_no_match_50_50_rollout(
        #[case] segment_rules: Vec<TargetingRule>,
        #[case] entity_attributes: HashMap<String, Value>,
    ) {
        let feature = {
            let inner_feature = crate::models::Feature {
                name: "F1".to_string(),
                feature_id: "f1".to_string(),
                kind: ValueKind::Numeric,
                _format: None,
                enabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                disabled_value: ConfigValue(serde_json::Value::Number((2).into())),
                segment_rules,
                enabled: true,
                rollout_percentage: 50,
            };
            let enabled_value = (inner_feature.kind, inner_feature.enabled_value.clone())
                .try_into()
                .unwrap();
            let disabled_value = (inner_feature.kind, inner_feature.disabled_value.clone())
                .try_into()
                .unwrap();
            let segment_rules = SegmentRules::new(
                HashMap::new(),
                inner_feature.segment_rules.clone(),
                inner_feature.kind,
            );
            FeatureSnapshot::new(
                inner_feature.enabled,
                enabled_value,
                disabled_value,
                inner_feature.rollout_percentage,
                &inner_feature.name,
                &inner_feature.feature_id,
                segment_rules,
            )
        };

        // One entity and feature combination which leads to no rollout:
        let entity = crate::tests::GenericEntity {
            id: "a1".into(),
            attributes: entity_attributes.clone(),
        };
        assert_eq!(
            random_value(format!("{}:{}", entity.id, feature.feature_id).as_str()),
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
            random_value(format!("{}:{}", entity.id, feature.feature_id).as_str()),
            29
        );
        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }

    // If the feature is disabled, always the disabled value should be returned.
    #[test]
    fn test_get_value_disabled_feature() {
        let feature = {
            let inner_feature = crate::models::Feature {
                name: "F1".to_string(),
                feature_id: "f1".to_string(),
                kind: ValueKind::Numeric,
                _format: None,
                enabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                disabled_value: ConfigValue(serde_json::Value::Number((2).into())),
                segment_rules: Vec::new(),
                enabled: false,
                rollout_percentage: 100,
            };
            let enabled_value = (inner_feature.kind, inner_feature.enabled_value.clone())
                .try_into()
                .unwrap();
            let disabled_value = (inner_feature.kind, inner_feature.disabled_value.clone())
                .try_into()
                .unwrap();
            let segment_rules = SegmentRules::new(
                HashMap::new(),
                inner_feature.segment_rules.clone(),
                inner_feature.kind,
            );
            FeatureSnapshot::new(
                inner_feature.enabled,
                enabled_value,
                disabled_value,
                inner_feature.rollout_percentage,
                &inner_feature.name,
                &inner_feature.feature_id,
                segment_rules,
            )
        };

        let entity = crate::entity::tests::TrivialEntity {};
        let value = feature.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &2));
    }

    // Get a feature value using different entities, matching or not matching a segment rule.
    // Uses rollout percentage to also test no rollout even if matched
    #[test]
    fn test_get_value_matching_a_rule() {
        let feature = {
            let inner_feature = crate::models::Feature {
                name: "F1".to_string(),
                feature_id: "f1".to_string(),
                kind: ValueKind::Numeric,
                _format: None,
                enabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                disabled_value: ConfigValue(serde_json::Value::Number((2).into())),
                segment_rules: vec![TargetingRule {
                    rules: vec![Segments {
                        segments: vec!["some_segment_id".into()],
                    }],
                    value: ConfigValue(serde_json::Value::Number((-48).into())),
                    order: 0,
                    rollout_percentage: Some(ConfigValue(serde_json::Value::Number((50).into()))),
                }],
                enabled: true,
                rollout_percentage: 50,
            };
            let enabled_value = (inner_feature.kind, inner_feature.enabled_value.clone())
                .try_into()
                .unwrap();
            let disabled_value = (inner_feature.kind, inner_feature.disabled_value.clone())
                .try_into()
                .unwrap();
            let segments = HashMap::from([(
                "some_segment_id".into(),
                Segment {
                    _name: "".into(),
                    segment_id: "".into(),
                    _description: "".into(),
                    _tags: None,
                    rules: vec![SegmentRule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = SegmentRules::new(
                segments,
                inner_feature.segment_rules.clone(),
                inner_feature.kind,
            );
            FeatureSnapshot::new(
                inner_feature.enabled,
                enabled_value,
                disabled_value,
                inner_feature.rollout_percentage,
                &inner_feature.name,
                &inner_feature.feature_id,
                segment_rules,
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
            let inner_feature = crate::models::Feature {
                name: "F1".to_string(),
                feature_id: "f1".to_string(),
                kind: ValueKind::Numeric,
                _format: None,
                enabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                disabled_value: ConfigValue(serde_json::Value::Number((2).into())),
                segment_rules: vec![TargetingRule {
                    rules: vec![Segments {
                        segments: vec!["some_segment_id".into()],
                    }],
                    value: ConfigValue(serde_json::Value::String("$default".into())),
                    order: 0,
                    rollout_percentage: Some(ConfigValue(serde_json::Value::Number((50).into()))),
                }],
                enabled: true,
                rollout_percentage: 50,
            };
            let enabled_value = (inner_feature.kind, inner_feature.enabled_value.clone())
                .try_into()
                .unwrap();
            let disabled_value = (inner_feature.kind, inner_feature.disabled_value.clone())
                .try_into()
                .unwrap();
            let segments = HashMap::from([(
                "some_segment_id".into(),
                Segment {
                    _name: "".into(),
                    segment_id: "".into(),
                    _description: "".into(),
                    _tags: None,
                    rules: vec![SegmentRule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = SegmentRules::new(
                segments,
                inner_feature.segment_rules.clone(),
                inner_feature.kind,
            );
            FeatureSnapshot::new(
                inner_feature.enabled,
                enabled_value,
                disabled_value,
                inner_feature.rollout_percentage,
                &inner_feature.name,
                &inner_feature.feature_id,
                segment_rules,
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
            let inner_feature = crate::models::Feature {
                name: "F1".to_string(),
                feature_id: "f1".to_string(),
                kind: ValueKind::Numeric,
                _format: None,
                enabled_value: ConfigValue(serde_json::Value::Number((-42).into())),
                disabled_value: ConfigValue(serde_json::Value::Number((2).into())),
                segment_rules: vec![TargetingRule {
                    rules: vec![Segments {
                        segments: vec!["some_segment_id".into()],
                    }],
                    value: ConfigValue(serde_json::Value::Number((48).into())),
                    order: 0,
                    rollout_percentage: Some(ConfigValue(serde_json::Value::String(
                        "$default".into(),
                    ))),
                }],
                enabled: true,
                rollout_percentage: 0,
            };
            let enabled_value = (inner_feature.kind, inner_feature.enabled_value.clone())
                .try_into()
                .unwrap();
            let disabled_value = (inner_feature.kind, inner_feature.disabled_value.clone())
                .try_into()
                .unwrap();
            let segments = HashMap::from([(
                "some_segment_id".into(),
                Segment {
                    _name: "".into(),
                    segment_id: "".into(),
                    _description: "".into(),
                    _tags: None,
                    rules: vec![SegmentRule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = SegmentRules::new(
                segments,
                inner_feature.segment_rules.clone(),
                inner_feature.kind,
            );
            FeatureSnapshot::new(
                inner_feature.enabled,
                enabled_value,
                disabled_value,
                inner_feature.rollout_percentage,
                &inner_feature.name,
                &inner_feature.feature_id,
                segment_rules,
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
}

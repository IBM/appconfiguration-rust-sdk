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
use crate::errors::Result;
use crate::metering::{MeteringRecorderSender, MeteringSubject};
use crate::models::{ROLLOUT_TYPE_PROGRESSIVE, RolloutConfiguration};
use crate::segment_evaluation::TargetingRules;
use crate::utils::{get_current_rollout_percentage, parse_rollout_configuration_phases};
use crate::value::Value;
use crate::{Feature, FeatureEvaluationDetails, FeatureEvaluationResult};
use chrono::Utc;
use murmur3::murmur3_32;
use std::collections::BTreeMap;
use std::io::Cursor;

/// Provides a snapshot of a [`Feature`].
#[derive(Debug)]
pub struct FeatureSnapshot {
    enabled: bool,
    enabled_value: Value,
    disabled_value: Value,
    rollout_percentage: u32,
    rollout_type: Option<String>,
    rollout_configuration: Option<RolloutConfiguration>,
    /// BTreeMap for feature-level progressive rollout (timestamp -> percentage)
    rollout_btree: Option<BTreeMap<i64, u32>>,
    pub(crate) name: String,
    pub(crate) feature_id: String,
    r#type: String,
    format: Option<String>,
    segment_rules: TargetingRules,
    pub(crate) metering: Option<MeteringRecorderSender>,
}

impl FeatureSnapshot {
    pub(crate) fn new(
        enabled: bool,
        enabled_value: Value,
        disabled_value: Value,
        rollout_percentage: u32,
        rollout_type: Option<String>,
        rollout_configuration: Option<RolloutConfiguration>,
        name: &str,
        feature_id: &str,
        r#type: String,
        format: Option<String>,
        segment_rules: TargetingRules,
        metering: Option<MeteringRecorderSender>,
    ) -> Self {
        let rollout_btree = if rollout_type.as_deref() == Some(ROLLOUT_TYPE_PROGRESSIVE) {
            rollout_configuration
                .as_ref()
                .and_then(|config| parse_rollout_configuration_phases(config).ok())
        } else {
            None
        };

        Self {
            enabled,
            enabled_value,
            disabled_value,
            rollout_percentage,
            rollout_type,
            rollout_configuration,
            rollout_btree,
            name: name.to_string(),
            feature_id: feature_id.to_string(),
            r#type,
            format,
            segment_rules,
            metering,
        }
    }

    fn evaluate_feature_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<(Value, bool, FeatureEvaluationDetails)> {
        if !self.enabled {
            self.record_evaluation(entity, None);
            return Ok((
                self.disabled_value.clone(),
                false,
                FeatureEvaluationDetails {
                    value_type: "DISABLED_VALUE".to_string(),
                    reason: "Feature is disabled. Returning disabled value.".to_string(),
                    segment_name: None,
                    rollout_percentage_applied: None,
                },
            ));
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
                let segment_name = segment.map(|s| s.name.clone());
                // Should rollout?
                // For segment-level progressive rollout the hash input uses entityId+start_at
                // rollout_config_map and surfaced through segment_rule.entity_id_for_hash().
                let entity_id_for_hash = segment_rule.entity_id_for_hash(entity.get_id());

                if Self::should_rollout_with_id(
                    rollout_percentage,
                    &entity_id_for_hash,
                    &self.feature_id,
                ) {
                    let value = segment_rule.value(&self.enabled_value)?;
                    Ok((
                        value,
                        true,
                        FeatureEvaluationDetails {
                            value_type: "SEGMENT_VALUE".to_string(),
                            reason: format!(
                                "Matched targeting rule order {} and rollout {}% allowed entity.",
                                segment_rule.order(),
                                rollout_percentage
                            ),
                            segment_name,
                            rollout_percentage_applied: Some(true),
                        },
                    ))
                } else {
                    Ok((
                        self.disabled_value.clone(),
                        false,
                        FeatureEvaluationDetails {
                            value_type: "DISABLED_VALUE".to_string(),
                            reason: format!(
                                "Matched targeting rule order {} but rollout {}% excluded entity.",
                                segment_rule.order(),
                                rollout_percentage
                            ),
                            segment_name,
                            rollout_percentage_applied: Some(false),
                        },
                    ))
                }
            }
            None => {
                let (effective_percentage, entity_id_for_hash) =
                    self.get_feature_rollout_percentage_and_entity_id(entity);

                let is_enabled = Self::should_rollout_with_id(
                    effective_percentage,
                    &entity_id_for_hash,
                    &self.feature_id,
                );
                let value = if is_enabled {
                    self.enabled_value.clone()
                } else {
                    self.disabled_value.clone()
                };

                let (value_type, reason, rollout_percentage_applied) = if is_enabled {
                    (
                        "ENABLED_VALUE".to_string(),
                        format!(
                            "No targeting rule matched. Feature-level rollout {}% enabled entity.",
                            effective_percentage
                        ),
                        Some(true),
                    )
                } else {
                    (
                        "DISABLED_VALUE".to_string(),
                        format!(
                            "No targeting rule matched. Feature-level rollout {}% excluded entity.",
                            effective_percentage
                        ),
                        Some(false),
                    )
                };

                Ok((
                    value,
                    is_enabled,
                    FeatureEvaluationDetails {
                        value_type,
                        reason,
                        segment_name: None,
                        rollout_percentage_applied,
                    },
                ))
            }
        }
    }

    fn normalized_hash(data: &str) -> u32 {
        let hash = murmur3_32(&mut Cursor::new(data), 0).expect("Cannot hash the value.");
        (f64::from(hash) / f64::from(u32::MAX) * 100.0) as u32
    }

    fn should_rollout_with_id(rollout_percentage: u32, entity_id: &str, feature_id: &str) -> bool {
        let tag = format!("{}:{}", entity_id, feature_id);
        rollout_percentage == 100 || Self::normalized_hash(&tag) < rollout_percentage
    }

    fn get_feature_rollout_percentage_and_entity_id(&self, entity: &impl Entity) -> (u32, String) {
        if self.rollout_type.as_deref() == Some(ROLLOUT_TYPE_PROGRESSIVE) {
            if let Some(rollout_config) = &self.rollout_configuration {
                if let Some(btree) = &self.rollout_btree {
                    let current_time_ms = Utc::now().timestamp_millis();
                    let current_percentage = get_current_rollout_percentage(btree, current_time_ms);
                    // Append start_at to entity ID for stable bucket assignment
                    let modified_entity_id =
                        format!("{}{}", entity.get_id(), rollout_config.start_at);
                    return (current_percentage, modified_entity_id);
                }
            }
        }

        // Manual rollout — plain entity ID
        (self.rollout_percentage, entity.get_id())
    }
}

impl Feature for FeatureSnapshot {
    fn get_feature_name(&self) -> Result<String> {
        Ok(self.name.clone())
    }

    fn is_enabled(&self) -> Result<bool> {
        Ok(self.enabled)
    }
    fn get_feature_id(&self) -> Result<String> {
        Ok(self.feature_id.clone())
    }

    fn get_feature_data_type(&self) -> Result<String> {
        Ok(self.r#type.clone())
    }

    fn get_feature_data_format(&self) -> Result<Option<String>> {
        // If the Format is null or undefined for a String type, we default it to TEXT
        if self.format.is_none() && self.r#type == "STRING" {
            return Ok(Some("TEXT".to_string()));
        }
        Ok(self.format.clone())
    }

    fn get_current_value(&self, entity: &impl Entity) -> Result<FeatureEvaluationResult> {
        let (value, is_enabled, details) = self.evaluate_feature_for_entity(entity)?;
        Ok(FeatureEvaluationResult {
            value,
            is_enabled,
            details,
        })
    }

    fn get_value_into<T: TryFrom<Value, Error = crate::Error>>(
        &self,
        entity: &impl Entity,
    ) -> Result<T> {
        let value = self.get_current_value(entity).map(|r| r.value)?;
        value.try_into()
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use crate::feature::Feature;
    use crate::network::serialization::fixtures::{create_one_segment_rule, one_segment_rule};
    use crate::network::serialization::{Rule, Segment, SegmentRule, ValueType};
    use rstest::rstest;
    use std::collections::HashMap;

    #[rstest]
    #[case("a1", false)]
    #[case("a2", true)]
    fn test_should_rollout(#[case] entity_id: &str, #[case] partial_rollout_expectation: bool) {
        let result = FeatureSnapshot::should_rollout_with_id(100, entity_id, "f1");
        assert!(result);

        let result = FeatureSnapshot::should_rollout_with_id(0, entity_id, "f1");
        assert!(!result);

        let result = FeatureSnapshot::should_rollout_with_id(50, entity_id, "f1");
        assert_eq!(result, partial_rollout_expectation);

        let result = FeatureSnapshot::should_rollout_with_id(50, entity_id, "f4");
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
                TargetingRules::new(HashMap::new(), segment_rules, ValueType::Numeric, None);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                50,
                None,
                None,
                "F1",
                "f1",
                "NUMERIC".to_string(),
                None,
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
        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &2));

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
        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &(-42)));
    }

    // If the feature is disabled, always the disabled value should be returned.
    #[test]
    fn test_get_value_disabled_feature() {
        let feature = {
            let segment_rules =
                TargetingRules::new(HashMap::new(), Vec::new(), ValueType::Numeric, None);
            FeatureSnapshot::new(
                false,
                Value::Int64(-42),
                Value::Int64(2),
                100,
                None,
                None,
                "F1",
                "f1",
                "NUMERIC".to_string(),
                None,
                segment_rules,
                None,
            )
        };

        let entity = crate::entity::tests::TrivialEntity {};
        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &2));
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
            let segment_rules =
                TargetingRules::new(segments, one_segment_rule, ValueType::Numeric, None);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                50,
                None,
                None,
                "F1",
                "f1",
                "NUMERIC".to_string(),
                None,
                segment_rules,
                None,
            )
        };

        // matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &(-48)));

        // matching the segment + rollout disallowed
        let entity = crate::tests::GenericEntity {
            id: "a1".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &2));

        // not matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinzz".to_string()))]),
        };

        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &(-42)));
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
            let segment_rules =
                TargetingRules::new(segments, segment_rules, ValueType::Numeric, None);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                50,
                None,
                None,
                "F1",
                "f1",
                "NUMERIC".to_string(),
                None,
                segment_rules,
                None,
            )
        };

        // matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &(-42)));
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
            let segment_rules =
                TargetingRules::new(segments, segment_rules, ValueType::Numeric, None);
            FeatureSnapshot::new(
                true,
                Value::Int64(-42),
                Value::Int64(2),
                0,
                None,
                None,
                "F1",
                "f1",
                "NUMERIC".to_string(),
                None,
                segment_rules,
                None,
            )
        };

        // matching the segment + rollout allowed
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        let value = feature.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &2));
    }

    /// This test ensures that the rust client is using the same hashing algorithm as to other clients.
    /// See same test for Node client:
    /// https://github.com/IBM/appconfiguration-node-sdk/blob/master/test/unit/configurations/internal/utils.test.js#L25
    #[test]
    fn test_normalized_hash() {
        assert_eq!(FeatureSnapshot::normalized_hash("entityId:featureId"), 41)
    }

    /// Verify that progressive rollout uses the BTree percentage (time-based) and NOT the
    /// static `rollout_percentage` field, and that the entity hash input is
    #[test]
    fn test_progressive_rollout_uses_btree_percentage_not_static() {
        use crate::models::{RolloutConfiguration, RolloutPhase};

        // start_at is far in the past so we are well into phase 1 (10%)
        let past_start = "2000-01-01T00:00:00Z";
        let rollout_config = RolloutConfiguration {
            start_at: past_start.to_string(),
            phases: vec![
                RolloutPhase {
                    percentage: 10,
                    duration: Some(1),
                    duration_type: Some("days".to_string()),
                },
                RolloutPhase {
                    percentage: 100,
                    duration: None,
                    duration_type: None,
                },
            ],
        };

        // Feature has static rollout_percentage=100 (would pass everyone if used),
        // but rollout_type=PROGRESSIVE and a rollout_configuration with 100% phase
        // because start is long in the past, the BTree will return 100%.
        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            100, // static — this must NOT be the effective value for progressive
            Some("PROGRESSIVE".to_string()),
            Some(rollout_config),
            "Test Feature",
            "test_feat",
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::new(),
                vec![],
                crate::network::serialization::ValueType::Boolean,
                None,
            ),
            None,
        );

        // Any entity should get enabled_value because current BTree percentage is 100%
        let entity = crate::tests::GenericEntity {
            id: "any_user".into(),
            attributes: HashMap::new(),
        };
        let result = feature.get_current_value(&entity).unwrap();
        assert!(
            result.is_enabled,
            "Expected isEnabled=true for 100% progressive rollout"
        );
        assert!(matches!(result.value, Value::Boolean(true)));
    }

    /// Verify that when a progressive rollout has 0% at the current time (start far in future),
    /// no entities are enabled — even if static rollout_percentage is 100.
    #[test]
    fn test_progressive_rollout_zero_percent_before_start() {
        use crate::models::{RolloutConfiguration, RolloutPhase};

        // start_at is far in the future — BTree entry for "now" falls back to key=0 → 0%
        let future_start = "2099-01-01T00:00:00Z";
        let rollout_config = RolloutConfiguration {
            start_at: future_start.to_string(),
            phases: vec![RolloutPhase {
                percentage: 100,
                duration: None,
                duration_type: None,
            }],
        };

        // Static rollout_percentage=100 — if used directly, everyone would pass.
        // Progressive rollout should override this to 0% (before start).
        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            100,
            Some("PROGRESSIVE".to_string()),
            Some(rollout_config),
            "Future Feature",
            "future_feat",
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::new(),
                vec![],
                crate::network::serialization::ValueType::Boolean,
                None,
            ),
            None,
        );

        // No entity should be enabled — current time is before start_at, BTree returns 0%
        let entity = crate::tests::GenericEntity {
            id: "early_user".into(),
            attributes: HashMap::new(),
        };
        let result = feature.get_current_value(&entity).unwrap();
        assert!(
            !result.is_enabled,
            "Expected is_enabled=false before progressive rollout start: got {:?}",
            result
        );
        assert!(matches!(result.value, Value::Boolean(false)));
    }

    #[test]
    fn test_manual_rollout_ignores_rollout_configuration_metadata() {
        use crate::models::{RolloutConfiguration, RolloutPhase};

        let future_rollout_config = RolloutConfiguration {
            start_at: "2099-01-01T00:00:00Z".to_string(),
            phases: vec![RolloutPhase {
                percentage: 0,
                duration: None,
                duration_type: None,
            }],
        };

        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            100,
            Some("MANUAL".to_string()),
            Some(future_rollout_config),
            "Manual Feature",
            "manual_feat",
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::new(),
                vec![],
                crate::network::serialization::ValueType::Boolean,
                None,
            ),
            None,
        );

        let entity = crate::tests::GenericEntity {
            id: "manual_user".into(),
            attributes: HashMap::new(),
        };
        let result = feature.get_current_value(&entity).unwrap();

        assert!(
            result.is_enabled,
            "Expected manual rollout to use static rollout_percentage instead of rollout_configuration"
        );
        assert!(matches!(result.value, Value::Boolean(true)));
    }
}

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

use crate::analytics::analytics::AnalyticsSender;
use crate::analytics::record::MetricDescriptor;
use crate::entity::Entity;
use crate::errors::Result;
use crate::metering::{MeteringRecorderSender, MeteringSubject};
use crate::models::{ROLLOUT_TYPE_GUARDED, ROLLOUT_TYPE_PROGRESSIVE, RolloutConfiguration};
use crate::segment_evaluation::TargetingRules;
use crate::utils::{get_current_rollout_percentage, parse_rollout_configuration_phases};
use crate::value::Value;
use crate::{Feature, FeatureEvaluationDetails, FeatureEvaluationResult};
use chrono::Utc;
use murmur3::murmur3_32;
use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use log::warn;

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
    pub(crate) analytics: Option<AnalyticsSender>,
    /// Rollout ID for GUARDED features (absent on MANUAL/PROGRESSIVE).
    rollout_id: Option<String>,
    /// event_key → list of metric descriptors, built at parse time from `metrics[]`.
    metric_map: HashMap<String, Vec<MetricDescriptor>>,
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
        rollout_id: Option<String>,
        metric_map: HashMap<String, Vec<MetricDescriptor>>,
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
            analytics: None,
            rollout_id,
            metric_map,
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
                let rollout_percentage =
                    segment_rule.rollout_percentage(self.rollout_percentage)?;
                let segment_name = segment.map(|s| s.name.clone());

                // Build the hash key — depends on rollout type of the segment rule.
                // GUARDED  → entityId:featureId:rolloutId
                // PROGRESSIVE → entityId:featureId:start_at  (via entity_id_for_hash)
                // MANUAL/default → entityId (via entity_id_for_hash)
                let entity_id_for_hash = segment_rule
                    .entity_id_for_hash(entity.get_id(), &self.feature_id);
                // entity_id_for_hash already contains the full key (entityId:featureId[:suffix])
                // for all rollout types — do NOT append :featureId again.
                let normalised = Self::normalized_hash(&entity_id_for_hash);

                // For GUARDED segment rules, emit the evaluation analytics entry.
                if segment_rule.rollout_type() == Some(ROLLOUT_TYPE_GUARDED) {
                    if let Some(rid) = segment_rule.rollout_id() {
                        self.record_guarded_evaluation(
                            rid.to_string(),
                            entity.get_id(),
                            rollout_percentage,
                            normalised,
                        );
                    }
                }

                let in_rollout = rollout_percentage == 100 || normalised < rollout_percentage;
                if in_rollout {
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
                let (effective_percentage, hash_key) =
                    self.get_feature_rollout_percentage_and_entity_id(entity);

                // hash_key already contains the full key (entityId:featureId[:suffix])
                // for all rollout types — do NOT append :featureId again.
                let normalised = Self::normalized_hash(&hash_key);

                // For GUARDED feature-level rollout, emit the evaluation analytics entry.
                if self.rollout_type.as_deref() == Some(ROLLOUT_TYPE_GUARDED) {
                    if let Some(rid) = &self.rollout_id {
                        self.record_guarded_evaluation(
                            rid.clone(),
                            entity.get_id(),
                            effective_percentage,
                            normalised,
                        );
                    }
                }

                let is_enabled = effective_percentage == 100 || normalised < effective_percentage;
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

    /// Emit a GUARDED_EVALUATION analytics event, classifying the entity into
    /// target / original / outside per the Python `add_guarded_evaluation_entry` logic.
    fn record_guarded_evaluation(
        &self,
        rollout_id: String,
        entity_id: String,
        rollout_percentage: u32,
        normalised_value: u32,
    ) {
        // target:   0 <= norm < rollout_pct
        // original: rollout_pct <= norm < 2 * rollout_pct
        // outside:  norm >= 2 * rollout_pct  → not tracked
        // Cast to u64 before doubling to avoid any u32 overflow (safe for pct in 0..=100).
        let value_served = if (normalised_value as u64) < (rollout_percentage as u64) {
            "target"
        } else if (normalised_value as u64) < (2 * rollout_percentage as u64) {
            "original"
        } else {
            return; // outside both groups — do not track
        };

        if let Some(sender) = &self.analytics {
            sender.send_guarded_evaluation(
                rollout_id,
                entity_id,
                value_served.to_string(),
            );
        }
    }

    fn normalized_hash(data: &str) -> u32 {
        let hash = murmur3_32(&mut Cursor::new(data), 0).expect("Cannot hash the value.");
        // Python uses `max_hash_value = pow(2, 32) = 4_294_967_296` — NOT u32::MAX.
        // Using u32::MAX here would give 100 for hash == u32::MAX, pushing one entity
        // outside the [0, 99] range and breaking group assignment determinism.
        const MAX_HASH_VALUE: f64 = 4_294_967_296.0; // 2^32, same as Python
        (f64::from(hash) / MAX_HASH_VALUE * 100.0) as u32
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn should_rollout_with_id(rollout_percentage: u32, entity_id: &str, feature_id: &str) -> bool {
        let tag = format!("{}:{}", entity_id, feature_id);
        rollout_percentage == 100 || Self::normalized_hash(&tag) < rollout_percentage
    }

    fn get_feature_rollout_percentage_and_entity_id(&self, entity: &impl Entity) -> (u32, String) {
        match self.rollout_type.as_deref() {
            Some(ROLLOUT_TYPE_PROGRESSIVE) => {
                if let Some(rollout_config) = &self.rollout_configuration {
                    if let Some(btree) = &self.rollout_btree {
                        let current_time_ms = Utc::now().timestamp_millis();
                        let current_percentage = get_current_rollout_percentage(btree, current_time_ms);
                        // entityId:featureId:start_at for stable progressive bucket
                        let key = format!(
                            "{}:{}:{}",
                            entity.get_id(),
                            self.feature_id,
                            rollout_config.start_at
                        );
                        return (current_percentage, key);
                    }
                }
                // btree is missing (parse failed or rollout_configuration absent):
                // Python returns rollout_percentage=0 here (rollout_map is None → 0%).
                // Use 0 so no entity is enabled — safe/conservative fallback.
                (0, entity.get_id())
            }
            Some(ROLLOUT_TYPE_GUARDED) => {
                // entityId:featureId:rolloutId
                let key = format!(
                    "{}:{}:{}",
                    entity.get_id(),
                    self.feature_id,
                    self.rollout_id.as_deref().unwrap_or("")
                );
                (self.rollout_percentage, key)
            }
            _ => {
                // MANUAL or unset — entityId:featureId
                let key = format!("{}:{}", entity.get_id(), self.feature_id);
                (self.rollout_percentage, key)
            }
        }
    }

    /// Track a business metric event for a GUARDED feature.
    ///
    /// Mirrors Python's `Feature.track(event_key, entity_id)`.
    pub fn track(&self, event_key: &str, entity_id: &str) {
        if entity_id.trim().is_empty() {
            warn!("track(): entity_id must be a non-empty string");
            return;
        }
        if event_key.trim().is_empty() {
            warn!("track(): event_key must be a non-empty string");
            return;
        }

        // Determine rollout_id, rollout_percentage, and optional segment-level metric_map.
        let (rollout_id, rollout_percentage, seg_metric_map) = if self.rollout_type.as_deref()
            == Some(ROLLOUT_TYPE_GUARDED)
        {
            match &self.rollout_id {
                Some(rid) => (rid.clone(), self.rollout_percentage, None),
                None => {
                    warn!("track(): feature is GUARDED but has no rollout_id");
                    return;
                }
            }
        } else {
            // Look for a GUARDED segment rule.
            // Pass feature-level rollout_percentage as the fallback for "$default" rules,
            // matching Python: `rollout_percentage = self.get_rollout_percentage()`.
            match self
                .segment_rules
                .find_guarded_segment_rule(self.rollout_percentage)
            {
                Some((rid, pct, seg_mm)) => (rid, pct, seg_mm),
                None => {
                    warn!(
                        "track(): feature '{}' is not configured for guarded rollout",
                        self.feature_id
                    );
                    return;
                }
            }
        };

        // Resolve metric_map: segment-level map takes priority when the event_key is present
        // in it — mirrors Python: `if rule_metric_map and event_key in rule_metric_map`.
        let resolved_metrics = if let Some(ref seg_mm) = seg_metric_map {
            if let Some(m) = seg_mm.get(event_key) {
                Some(m.clone())
            } else {
                self.metric_map.get(event_key).cloned()
            }
        } else {
            self.metric_map.get(event_key).cloned()
        };

        let metrics = match resolved_metrics {
            Some(m) => m,
            None => {
                warn!(
                    "track(): event_key '{}' not found in metric_map for feature '{}'",
                    event_key, self.feature_id
                );
                return;
            }
        };

        // Normalised value key: entityId:featureId:rolloutId
        let key = format!("{}:{}:{}", entity_id, self.feature_id, rollout_id);
        let normalised = Self::normalized_hash(&key);

        let value_served = if (normalised as u64) < (rollout_percentage as u64) {
            "target"
        } else if (normalised as u64) < (2 * rollout_percentage as u64) {
            "original"
        } else {
            return; // outside — not tracked
        };

        if let Some(sender) = &self.analytics {
            sender.send_guarded_metric(
                rollout_id,
                entity_id.to_string(),
                value_served.to_string(),
                event_key.to_string(),
                metrics,
            );
        }
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

    fn track(&self, event_key: &str, entity_id: &str) {
        FeatureSnapshot::track(self, event_key, entity_id);
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
                None,
                HashMap::new(),
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
                None,
                HashMap::new(),
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
                None,
                HashMap::new(),
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
                None,
                HashMap::new(),
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
                None,
                HashMap::new(),
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
            None,
            HashMap::new(),
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
            None,
            HashMap::new(),
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
            None,
            HashMap::new(),
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

    // ════════════════════════════════════════════════════════════════════════════
    // GUARDED ROLLOUT — feature-level evaluation
    // Mirrors TestConfigurationHandlerFeatureLevelGuarded in test_guarded_rollout.py
    //
    // Deterministic murmur3 hashes for key "entityId:gf-1:rid-1", rollout_percentage=30:
    //   target   (norm < 30):  u17=19, u19=2, u21=4, u22=22, u25=9
    //   original (30≤norm<60): u1=32,  u5=41, u8=33, u10=33, u11=58
    //   outside  (norm≥60):    u2=91,  u3=65, u4=75, u6=67,  u7=95
    // ════════════════════════════════════════════════════════════════════════════

    const GF1: &str = "gf-1";
    const RID1: &str = "rid-1";
    const SEG_RID1: &str = "seg-rid-1";
    const SEG1: &str = "seg-1";
    const ROLLOUT_PCT: u32 = 30;

    fn make_guarded_feature() -> FeatureSnapshot {
        make_guarded_feature_pct(ROLLOUT_PCT)
    }

    fn make_guarded_feature_pct(pct: u32) -> FeatureSnapshot {
        FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            pct,
            Some("GUARDED".to_string()),
            None,
            "Guarded Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(HashMap::new(), vec![], ValueType::Boolean, None),
            None,
            Some(RID1.to_string()),
            HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]),
            ]),
        )
    }

    fn make_guarded_segment_feature() -> FeatureSnapshot {
        // Feature-level is MANUAL/100%; one GUARDED segment rule.
        use crate::network::serialization::Segment;
        use crate::network::serialization::fixtures::{ConfigValue, Segments};
        let seg_rule = SegmentRule {
            rules: vec![Segments { segments: vec![SEG1.to_string()] }],
            value: ConfigValue(serde_json::Value::String("$default".into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number(ROLLOUT_PCT.into()))),
            rule_id: None,
            rollout_type: Some("GUARDED".to_string()),
            rollout_configuration: None,
            rollout_id: Some(SEG_RID1.to_string()),
            metric_map: None,
        };
        let segment = Segment {
            name: "Open Seg".to_string(),
            segment_id: SEG1.to_string(),
            description: None,
            tags: None,
            rules: vec![crate::network::serialization::Rule {
                attribute_name: "open".to_string(),
                operator: "is".to_string(),
                values: vec!["yes".to_string()],
            }],
        };
        let segments = HashMap::from([(SEG1.to_string(), segment)]);
        FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            100,
            Some("MANUAL".to_string()),
            None,
            "Guarded Seg Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(segments, vec![seg_rule], ValueType::Boolean, None),
            None,
            None,
            HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]),
            ]),
        )
    }

    fn entity_with_open_attr(id: &str) -> crate::tests::GenericEntity {
        crate::tests::GenericEntity {
            id: id.to_string(),
            attributes: HashMap::from([("open".to_string(), Value::from("yes".to_string()))]),
        }
    }

    fn plain_entity(id: &str) -> crate::tests::GenericEntity {
        crate::tests::GenericEntity { id: id.to_string(), attributes: HashMap::new() }
    }

    // ── hash sanity (matches Python comment at top of test_guarded_rollout.py) ──

    #[test]
    fn test_guarded_hash_values_match_python_reference() {
        // key = "entityId:gf-1:rid-1"
        let hash = |eid: &str| FeatureSnapshot::normalized_hash(
            &format!("{}:{}:{}", eid, GF1, RID1)
        );
        // target range (< 30)
        assert_eq!(hash("u17"), 19);
        assert_eq!(hash("u19"), 2);
        assert_eq!(hash("u21"), 4);
        assert_eq!(hash("u22"), 22);
        assert_eq!(hash("u25"), 9);
        // original range (30 ≤ norm < 60)
        assert_eq!(hash("u1"), 32);
        assert_eq!(hash("u5"), 41);
        assert_eq!(hash("u8"), 33);
        // outside (≥ 60)
        assert_eq!(hash("u2"), 91);
        assert_eq!(hash("u3"), 65);
        assert_eq!(hash("u4"), 75);
    }

    #[test]
    fn test_guarded_segment_hash_values_match_python_reference() {
        // key = "entityId:gf-1:seg-rid-1"
        let hash = |eid: &str| FeatureSnapshot::normalized_hash(
            &format!("{}:{}:{}", eid, GF1, SEG_RID1)
        );
        // target
        assert_eq!(hash("u2"), 11);
        assert_eq!(hash("u13"), 16);
        assert_eq!(hash("u14"), 24);
        assert_eq!(hash("u16"), 9);
        assert_eq!(hash("u17"), 6);
        // original
        assert_eq!(hash("u3"), 53);
        assert_eq!(hash("u6"), 42);
        assert_eq!(hash("u7"), 30);
        assert_eq!(hash("u8"), 58);
        assert_eq!(hash("u10"), 30);
        // outside
        assert_eq!(hash("u1"), 79);
        assert_eq!(hash("u4"), 88);
        assert_eq!(hash("u5"), 94);
    }

    // ── feature-level evaluation ──────────────────────────────────────────────

    /// test_serves_enabled_for_entity_in_target_range — u17: norm=19 < 30
    #[test]
    fn test_guarded_feature_level_serves_enabled_for_target_entity() {
        let f = make_guarded_feature();
        let result = f.get_current_value(&plain_entity("u17")).unwrap();
        assert!(result.is_enabled);
        assert!(matches!(result.value, Value::Boolean(true)));
    }

    /// test_serves_disabled_for_entity_in_original_range — u1: norm=32 → 30≤32<60
    #[test]
    fn test_guarded_feature_level_serves_disabled_for_original_entity() {
        let f = make_guarded_feature();
        let result = f.get_current_value(&plain_entity("u1")).unwrap();
        assert!(!result.is_enabled);
        assert!(matches!(result.value, Value::Boolean(false)));
    }

    /// test_serves_disabled_for_entity_outside_both_ranges — u2: norm=91 ≥ 60
    #[test]
    fn test_guarded_feature_level_serves_disabled_for_outside_entity() {
        let f = make_guarded_feature();
        let result = f.get_current_value(&plain_entity("u2")).unwrap();
        assert!(!result.is_enabled);
        assert!(matches!(result.value, Value::Boolean(false)));
    }

    /// test_serves_disabled_when_feature_flag_is_disabled
    #[test]
    fn test_guarded_feature_level_serves_disabled_when_feature_disabled() {
        let mut f = make_guarded_feature();
        f.enabled = false;
        let result = f.get_current_value(&plain_entity("u17")).unwrap();
        assert!(!result.is_enabled);
        assert!(matches!(result.value, Value::Boolean(false)));
    }

    /// test_rollout_100_always_serves_enabled — even for "outside" entity u2
    #[test]
    fn test_guarded_feature_level_rollout_100_always_enabled() {
        let f = make_guarded_feature_pct(100);
        let result = f.get_current_value(&plain_entity("u2")).unwrap();
        assert!(result.is_enabled);
    }

    // ── analytics capture helper ──────────────────────────────────────────────
    //
    // Pattern: feature gets a sender clone; the handle stays in the caller so
    // `handle.join()` can be used for clean teardown:
    //   1. call the method under test
    //   2. drop(f)      — drops feature's sender
    //   3. handle.join() — drops handle's sender → Disconnected → final flush
    //   4. assert on store

    type Store = std::sync::Arc<std::sync::Mutex<Vec<crate::analytics::record::AnalyticsPayload>>>;

    fn make_analytics_fixture(feature: FeatureSnapshot)
        -> (FeatureSnapshot, crate::analytics::analytics::AnalyticsHandle, Store)
    {
        use crate::analytics::analytics::start_analytics;
        use crate::ConfigurationId;
        use crate::analytics::client::AnalyticsClient;
        use crate::analytics::errors::AnalyticsResult;
        use crate::analytics::record::AnalyticsPayload;
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct Cap(Arc<Mutex<Vec<AnalyticsPayload>>>);
        impl AnalyticsClient for Cap {
            fn push_analytics_data(&self, _: &str, p: &AnalyticsPayload) -> AnalyticsResult<()> {
                self.0.lock().unwrap().push(p.clone()); Ok(())
            }
        }
        let store: Store = Arc::new(Mutex::new(vec![]));
        let handle = start_analytics(
            ConfigurationId::new("g".into(), "env".into(), "col".into()),
            Cap(store.clone()),
        );
        let mut f = feature;
        f.analytics = Some(handle.sender.clone());
        (f, handle, store)
    }

    /// Consume the feature and handle, wait for the final flush, return usages.
    fn drain_usages(
        f: FeatureSnapshot,
        handle: crate::analytics::analytics::AnalyticsHandle,
        store: Store,
    ) -> Vec<crate::analytics::record::AnalyticsUsage> {
        drop(f);        // drop feature's sender
        handle.join();  // drop handle's sender → Disconnected → final flush
        let g = store.lock().unwrap();
        if g.is_empty() { vec![] } else { g[0].usages.clone() }
    }

    /// Consume, join, assert nothing was sent.
    fn assert_nothing_flushed(
        f: FeatureSnapshot,
        handle: crate::analytics::analytics::AnalyticsHandle,
        store: Store,
    ) {
        drop(f);
        handle.join();
        assert!(store.lock().unwrap().is_empty(), "expected no analytics payload to be flushed");
    }

    // ── record_guarded_evaluation logic ──────────────────────────────────────

    /// test_records_target_when_norm_below_percentage — norm=19, pct=30 → target
    #[test]
    fn test_record_guarded_evaluation_target() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature_pct(ROLLOUT_PCT));
        f.record_guarded_evaluation("rid-1".into(), "u17".into(), ROLLOUT_PCT, 19);
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages[0].value_served, "target");
        assert_eq!(usages[0].rollout_id, "rid-1");
        assert_eq!(usages[0].entity_id, "u17");
        assert_eq!(usages[0].metadata.event_type, "evaluation");
    }

    /// test_records_original_when_norm_in_original_range — norm=32 → original
    #[test]
    fn test_record_guarded_evaluation_original() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature_pct(ROLLOUT_PCT));
        f.record_guarded_evaluation("rid-1".into(), "u1".into(), ROLLOUT_PCT, 32);
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages[0].value_served, "original");
    }

    /// test_records_nothing_when_norm_outside_both_ranges — norm=91 ≥ 60 → nothing
    #[test]
    fn test_record_guarded_evaluation_outside_emits_nothing() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature_pct(ROLLOUT_PCT));
        f.record_guarded_evaluation("rid-1".into(), "u2".into(), ROLLOUT_PCT, 91);
        assert_nothing_flushed(f, handle, store);
    }

    // ── segment-level guarded evaluation ─────────────────────────────────────

    /// test_serves_enabled_and_records_target_for_entity_in_target_range
    /// u2 with seg key: norm("u2:gf-1:seg-rid-1")=11 < 30 → target, enabled
    #[test]
    fn test_guarded_segment_level_serves_enabled_for_target_entity() {
        let f = make_guarded_segment_feature();
        let entity = entity_with_open_attr("u2");
        let result = f.get_current_value(&entity).unwrap();
        assert!(result.is_enabled);
        assert!(matches!(result.value, Value::Boolean(true)));
    }

    /// u3 with seg key: norm("u3:gf-1:seg-rid-1")=53 → 30≤53<60 → original, disabled
    #[test]
    fn test_guarded_segment_level_serves_disabled_for_original_entity() {
        let f = make_guarded_segment_feature();
        let entity = entity_with_open_attr("u3");
        let result = f.get_current_value(&entity).unwrap();
        assert!(!result.is_enabled);
        assert!(matches!(result.value, Value::Boolean(false)));
    }

    /// u1 with seg key: norm("u1:gf-1:seg-rid-1")=79 ≥ 60 → outside, disabled
    #[test]
    fn test_guarded_segment_level_serves_disabled_for_outside_entity() {
        let f = make_guarded_segment_feature();
        let entity = entity_with_open_attr("u1");
        let result = f.get_current_value(&entity).unwrap();
        assert!(!result.is_enabled);
    }

    // ── track() ───────────────────────────────────────────────────────────────

    /// test_emits_guarded_metric_target_for_entity_in_target_range — u17: norm=19 < 30
    #[test]
    fn test_track_emits_target_for_entity_in_target_range() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("purchase", "u17");
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].value_served, "target");
        assert_eq!(usages[0].rollout_id, RID1);
        assert_eq!(usages[0].entity_id, "u17");
        assert_eq!(usages[0].metadata.feature, "guarded");
        assert_eq!(usages[0].metadata.event_type, "metric");
    }

    /// test_emits_guarded_metric_original_for_entity_in_original_range — u1: norm=32
    #[test]
    fn test_track_emits_original_for_entity_in_original_range() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("purchase", "u1");
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].value_served, "original");
    }

    /// test_emits_nothing_for_entity_outside_both_ranges — u2: norm=91 ≥ 60
    #[test]
    fn test_track_emits_nothing_for_outside_entity() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("purchase", "u2");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_skips_when_entity_id_is_empty
    #[test]
    fn test_track_skips_empty_entity_id() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("purchase", "");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_skips_when_event_key_not_in_metric_map
    #[test]
    fn test_track_skips_unknown_event_key() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("unknown-event", "u17");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_skips_when_feature_not_configured_for_guarded (MANUAL, no segment rules)
    #[test]
    fn test_track_skips_non_guarded_feature() {
        let mut base = make_guarded_feature();
        base.rollout_type = Some("MANUAL".to_string());
        base.rollout_id = None;
        let (f, handle, store) = make_analytics_fixture(base);
        f.track("purchase", "u17");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_uses_segment_level_rollout_id_when_segment_is_guarded
    /// u2 with seg key: norm("u2:gf-1:seg-rid-1")=11 < 30 → target
    #[test]
    fn test_track_uses_segment_level_rollout_id() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_segment_feature());
        f.track("purchase", "u2"); // norm("u2:gf-1:seg-rid-1")=11 < 30 → target
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].value_served, "target");
        assert_eq!(usages[0].rollout_id, SEG_RID1);
    }

    /// test_track_includes_metrics_array_in_event_body — event_key stripped, metrics present
    #[test]
    fn test_track_strips_event_key_but_keeps_metrics() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("purchase", "u17");
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1);
        assert!(usages[0].event_key.is_none(), "event_key must be stripped from payload");
        let metrics = usages[0].metrics.as_ref().expect("metrics must be present");
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].id, "m1");
        assert_eq!(metrics[0].metric_type, "count");
    }

    /// test_track_segment_default_rollout_percentage_falls_back_to_feature_level
    ///
    /// Mirrors Python test_uses_segment_level_rollout_id_when_segment_is_guarded:
    ///   `if segment_rule.get('rollout_percentage') == DEFAULT_ROLLOUT_PERCENTAGE:
    ///        rollout_percentage = self.get_rollout_percentage()`
    ///
    /// Feature has rollout_percentage=30; segment rule has rollout_percentage="$default".
    /// The effective percentage must be 30 (from the feature), not 100 (the old hardcoded fallback).
    /// u2 with key "u2:gf-1:seg-rid-1" → norm=11 < 30 → target
    #[test]
    fn test_track_segment_default_rollout_percentage_falls_back_to_feature_level() {
        use crate::network::serialization::Segment;
        use crate::network::serialization::fixtures::{ConfigValue, Segments};

        // Feature is MANUAL with rollout_percentage=30.
        // Segment rule has rollout_percentage="$default" → should fall back to 30.
        let seg_rule = SegmentRule {
            rules: vec![Segments { segments: vec![SEG1.to_string()] }],
            value: ConfigValue(serde_json::Value::String("$default".into())),
            order: 0,
            // "$default" — the key Python comparison: rollout_percentage == DEFAULT_ROLLOUT_PERCENTAGE
            rollout_percentage: Some(ConfigValue(serde_json::Value::String("$default".into()))),
            rule_id: None,
            rollout_type: Some("GUARDED".to_string()),
            rollout_configuration: None,
            rollout_id: Some(SEG_RID1.to_string()),
            metric_map: None,
        };
        let segment = Segment {
            name: "Open Seg".to_string(),
            segment_id: SEG1.to_string(),
            description: None,
            tags: None,
            rules: vec![],
        };
        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            ROLLOUT_PCT, // feature-level pct = 30
            Some("MANUAL".to_string()),
            None,
            "Seg Default Pct Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::from([(SEG1.to_string(), segment)]),
                vec![seg_rule],
                ValueType::Boolean,
                None,
            ),
            None,
            None,
            HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]),
            ]),
        );

        let (f, handle, store) = make_analytics_fixture(feature);
        // u2: norm("u2:gf-1:seg-rid-1")=11 < 30 → target (would be target for pct=30 but not for pct=100)
        f.track("purchase", "u2");
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1, "u2 must be in target range when pct=30 is used (not 100)");
        assert_eq!(usages[0].value_served, "target");
        assert_eq!(usages[0].rollout_id, SEG_RID1);

        // u1: norm("u1:gf-1:seg-rid-1")=79 ≥ 60 → outside both groups (pct=30 → 2×30=60)
        let feature2 = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            ROLLOUT_PCT,
            Some("MANUAL".to_string()),
            None,
            "Seg Default Pct Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::from([(SEG1.to_string(), Segment {
                    name: "Open Seg".to_string(),
                    segment_id: SEG1.to_string(),
                    description: None,
                    tags: None,
                    rules: vec![],
                })]),
                vec![SegmentRule {
                    rules: vec![Segments { segments: vec![SEG1.to_string()] }],
                    value: ConfigValue(serde_json::Value::String("$default".into())),
                    order: 0,
                    rollout_percentage: Some(ConfigValue(serde_json::Value::String("$default".into()))),
                    rule_id: None,
                    rollout_type: Some("GUARDED".to_string()),
                    rollout_configuration: None,
                    rollout_id: Some(SEG_RID1.to_string()),
                    metric_map: None,
                }],
                ValueType::Boolean,
                None,
            ),
            None,
            None,
            HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]),
            ]),
        );
        let (f2, handle2, store2) = make_analytics_fixture(feature2);
        f2.track("purchase", "u1"); // norm=79 ≥ 60 → outside → nothing emitted
        assert_nothing_flushed(f2, handle2, store2);
    }

    // ── additional edge cases matching Python TestFeatureTrackEdgeCases ──────────

    /// test_skips_when_event_key_is_none / test_skips_when_event_key_is_whitespace
    /// Rust passes "" or "   " as event_key; both must be rejected.
    #[test]
    fn test_track_skips_whitespace_only_event_key() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("   ", "u17");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_skips_when_entity_id_is_whitespace
    #[test]
    fn test_track_skips_whitespace_only_entity_id() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_feature());
        f.track("purchase", "   ");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_segment_metric_map_takes_priority_over_feature_metric_map
    ///
    /// When a segment rule carries its own `metric_map`, `track()` must resolve
    /// the metrics from the segment map rather than the feature-level map.
    /// u2: norm("u2:gf-1:seg-rid-1")=11 < 30 → target
    #[test]
    fn test_track_segment_metric_map_takes_priority_over_feature_metric_map() {
        use crate::network::serialization::Segment;
        use crate::network::serialization::fixtures::{ConfigValue, Segments};

        // Segment rule has metric_map{"purchase":[{id:"seg-metric"}]}.
        // Feature-level map has {"purchase":[{id:"feat-metric"}]}.
        // track("u2", "purchase") must use the segment metric.
        let seg_rule = SegmentRule {
            rules: vec![Segments { segments: vec![SEG1.to_string()] }],
            value: ConfigValue(serde_json::Value::String("$default".into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number(ROLLOUT_PCT.into()))),
            rule_id: None,
            rollout_type: Some("GUARDED".to_string()),
            rollout_configuration: None,
            rollout_id: Some(SEG_RID1.to_string()),
            metric_map: Some(HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "seg-metric".into(), metric_type: "count".into() }]),
            ])),
        };
        // Segment with empty rules → always matches (no attribute check needed)
        let segment = Segment {
            name: "Open Seg".to_string(),
            segment_id: SEG1.to_string(),
            description: None,
            tags: None,
            rules: vec![],
        };
        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            ROLLOUT_PCT,
            Some("MANUAL".to_string()),
            None,
            "Seg Metric Map Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::from([(SEG1.to_string(), segment)]),
                vec![seg_rule],
                ValueType::Boolean,
                None,
            ),
            None,
            None,
            HashMap::from([
                // feature-level metric — must NOT be used when segment has its own
                ("purchase".to_string(), vec![MetricDescriptor { id: "feat-metric".into(), metric_type: "count".into() }]),
            ]),
        );

        let (f, handle, store) = make_analytics_fixture(feature);
        f.track("purchase", "u2"); // norm("u2:gf-1:seg-rid-1")=11 < 30 → target
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1);
        let metrics = usages[0].metrics.as_ref().expect("metrics must be present");
        assert_eq!(metrics[0].id, "seg-metric",
            "segment-level metric_map must take priority over feature-level metric_map");
    }

    /// test_segment_level_guarded_original_range
    /// u3: norm("u3:gf-1:seg-rid-1")=53 → 30≤53<60 → original
    #[test]
    fn test_track_segment_level_guarded_original_range() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_segment_feature());
        f.track("purchase", "u3"); // norm=53 → original
        let usages = drain_usages(f, handle, store);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].value_served, "original");
        assert_eq!(usages[0].rollout_id, SEG_RID1);
    }

    /// test_segment_level_guarded_outside_range_emits_nothing
    /// u1: norm("u1:gf-1:seg-rid-1")=79 ≥ 60 → outside
    #[test]
    fn test_track_segment_level_guarded_outside_range_emits_nothing() {
        let (f, handle, store) = make_analytics_fixture(make_guarded_segment_feature());
        f.track("purchase", "u1"); // norm=79 → outside
        assert_nothing_flushed(f, handle, store);
    }

    /// test_segment_progressive_missing_btree_emits_nothing
    ///
    /// When a segment rule has rollout_type=PROGRESSIVE but no matching btree entry
    /// in `rollout_config_map` (rollout_configuration absent → btree not parsed),
    /// `rollout_percentage()` must return 0.  With 0% rollout every entity's hash
    /// is ≥ 0 so nobody falls inside [0, 0) and no analytics record is emitted.
    ///
    /// Matches Python: segment_rollout_percentage = 0 when rollout_map is None.
    #[test]
    fn test_track_segment_progressive_missing_btree_emits_nothing() {
        use crate::network::serialization::Segment;
        use crate::network::serialization::fixtures::{ConfigValue, Segments};

        // PROGRESSIVE segment rule WITH a rule_id but NO rollout_configuration
        // → TargetingRules::new will not insert any btree into rollout_config_map
        // → rollout_percentage() must fall back to 0.
        let seg_rule = SegmentRule {
            rules: vec![Segments { segments: vec![SEG1.to_string()] }],
            value: ConfigValue(serde_json::Value::String("$default".into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number(50_u32.into()))),
            rule_id: Some("prog-rule-1".to_string()),
            rollout_type: Some("PROGRESSIVE".to_string()),
            rollout_configuration: None, // no phases → btree absent
            rollout_id: Some(SEG_RID1.to_string()),
            metric_map: None,
        };
        // Segment with empty rules → always matches any entity
        let segment = Segment {
            name: "Open Seg".to_string(),
            segment_id: SEG1.to_string(),
            description: None,
            tags: None,
            rules: vec![],
        };
        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            100,
            Some("MANUAL".to_string()),
            None,
            "Progressive Seg Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::from([(SEG1.to_string(), segment)]),
                vec![seg_rule],
                ValueType::Boolean,
                None,
            ),
            None,
            None,
            HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]),
            ]),
        );

        let (f, handle, store) = make_analytics_fixture(feature);
        // Any entity: effective rollout_percentage = 0 → hash always ≥ 0 → never in range
        f.track("purchase", "u17");
        f.track("purchase", "u1");
        assert_nothing_flushed(f, handle, store);
    }

    /// test_segment_progressive_missing_rule_id_emits_nothing
    ///
    /// When a segment rule has rollout_type=PROGRESSIVE but no `rule_id`,
    /// the key lookup in `rollout_config_map` is skipped entirely and
    /// `rollout_percentage()` must return 0 — same fallback as missing btree.
    #[test]
    fn test_track_segment_progressive_missing_rule_id_emits_nothing() {
        use crate::network::serialization::Segment;
        use crate::network::serialization::fixtures::{ConfigValue, Segments};

        let seg_rule = SegmentRule {
            rules: vec![Segments { segments: vec![SEG1.to_string()] }],
            value: ConfigValue(serde_json::Value::String("$default".into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number(50_u32.into()))),
            rule_id: None, // <-- no rule_id → key lookup skipped
            rollout_type: Some("PROGRESSIVE".to_string()),
            rollout_configuration: None,
            rollout_id: Some(SEG_RID1.to_string()),
            metric_map: None,
        };
        let segment = Segment {
            name: "Open Seg".to_string(),
            segment_id: SEG1.to_string(),
            description: None,
            tags: None,
            rules: vec![],
        };
        let feature = FeatureSnapshot::new(
            true,
            Value::Boolean(true),
            Value::Boolean(false),
            100,
            Some("MANUAL".to_string()),
            None,
            "Progressive No RuleId Feature",
            GF1,
            "BOOLEAN".to_string(),
            None,
            TargetingRules::new(
                HashMap::from([(SEG1.to_string(), segment)]),
                vec![seg_rule],
                ValueType::Boolean,
                None,
            ),
            None,
            None,
            HashMap::from([
                ("purchase".to_string(), vec![MetricDescriptor { id: "m1".into(), metric_type: "count".into() }]),
            ]),
        );

        let (f, handle, store) = make_analytics_fixture(feature);
        f.track("purchase", "u3");
        f.track("purchase", "u2");
        assert_nothing_flushed(f, handle, store);
    }
}

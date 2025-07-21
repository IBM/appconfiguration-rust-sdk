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
use crate::metering::MeteringRecorderSender;
use crate::value::Value;
use crate::Property;

use crate::errors::Result;
use crate::segment_evaluation::TargetingRules;
use log::warn;

/// Provides a snapshot of a [`Property`].
#[derive(Debug)]
pub struct PropertySnapshot {
    value: Value,
    segment_rules: TargetingRules,
    name: String,
    pub(crate) metering: Option<MeteringRecorderSender>,
}

impl PropertySnapshot {
    pub(crate) fn new(
        value: Value,
        segment_rules: TargetingRules,
        name: &str,
        metering: Option<MeteringRecorderSender>,
    ) -> Self {
        Self {
            value,
            segment_rules,
            name: name.to_string(),
            metering,
        }
    }

    fn evaluate_feature_for_entity(&self, entity: &impl Entity) -> Result<Value> {
        let segment_rule_and_segment = {
            if self.segment_rules.is_empty() || entity.get_attributes().is_empty() {
                // TODO: this makes only sense if there can be a rule which matches
                //       even on empty attributes
                // No match possible. Do not consider segment rules:
                None
            } else {
                self.segment_rules
                    .find_applicable_targeting_rule_and_segment_for_entity(entity)?
            }
        };

        if let Some(metering) = self.metering.as_ref() {
            if let Err(e) = metering.record_property_evaluation(
                &self.name,
                &entity.get_id(),
                segment_rule_and_segment
                    .as_ref()
                    .map(|(_, segment)| segment.name.as_str()),
            ) {
                warn!("Fail to enqueue metering data: {e}");
            }
        }

        match segment_rule_and_segment {
            Some((segment_rule, _)) => segment_rule.value(&self.value),
            None => Ok(self.value.clone()),
        }
    }
}

impl Property for PropertySnapshot {
    fn get_name(&self) -> Result<String> {
        Ok(self.name.clone())
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
    use crate::models::{ConfigValue, Rule, Segment, SegmentRule, Segments, ValueType};
    use std::collections::HashMap;

    #[test]
    fn test_get_value_segment_with_default_value() {
        let property = {
            let segments = HashMap::from([(
                "some_segment_id_1".into(),
                Segment {
                    name: "".into(),
                    segment_id: "".into(),
                    description: "".into(),
                    tags: None,
                    rules: vec![Rule {
                        attribute_name: "name".into(),
                        operator: "is".into(),
                        values: vec!["heinz".into()],
                    }],
                },
            )]);
            let segment_rules = TargetingRules::new(
                segments,
                vec![SegmentRule {
                    rules: vec![Segments {
                        segments: vec!["some_segment_id_1".into()],
                    }],
                    value: ConfigValue(serde_json::Value::String("$default".into())),
                    order: 1,
                    rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
                }],
                ValueType::Numeric,
            );
            PropertySnapshot::new(Value::Int64(-42), segment_rules, "F1", None)
        };

        // Both segment rules match. Expect the one with smaller order to be used:
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };
        let value = property.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }
}

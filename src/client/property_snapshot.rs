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
use crate::Property;
use std::collections::HashMap;

use crate::errors::Result;
use crate::segment_evaluation::find_applicable_segment_rule_for_entity;

/// Provides a snapshot of a [`Property`].
#[derive(Debug)]
pub struct PropertySnapshot {
    property: crate::models::Property,
    segments: HashMap<String, crate::models::Segment>,
}

impl PropertySnapshot {
    pub(crate) fn new(
        property: crate::models::Property,
        segments: HashMap<String, crate::models::Segment>,
    ) -> Self {
        Self { property, segments }
    }

    fn evaluate_feature_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<crate::models::ConfigValue> {
        if self.property.segment_rules.is_empty() || entity.get_attributes().is_empty() {
            // TODO: this makes only sense if there can be a rule which matches
            //       even on empty attributes
            // No match possible. Do not consider segment rules:
            return Ok(self.property.value.clone());
        }

        match find_applicable_segment_rule_for_entity(
            &self.segments,
            &self.property.segment_rules,
            entity,
        )? {
            Some(segment_rule) => {
                if segment_rule.value.is_default() {
                    Ok(self.property.value.clone())
                } else {
                    Ok(segment_rule.value.clone())
                }
            }
            None => Ok(self.property.value.clone()),
        }
    }
}

impl Property for PropertySnapshot {
    fn get_name(&self) -> Result<String> {
        Ok(self.property.name.clone())
    }

    fn get_value(&self, entity: &impl Entity) -> Result<Value> {
        let model_value = self.evaluate_feature_for_entity(entity)?;
        (self.property.kind, model_value).try_into()
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

    #[test]
    fn test_get_value_segment_with_default_value() {
        let inner_property = crate::models::Property {
            name: "F1".to_string(),
            property_id: "f1".to_string(),
            kind: ValueKind::Numeric,
            _format: None,
            value: ConfigValue(serde_json::Value::Number((-42).into())),
            segment_rules: vec![TargetingRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_1".into()],
                }],
                value: ConfigValue(serde_json::Value::String("$default".into())),
                order: 1,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            }],
            _tags: None,
        };
        let property = PropertySnapshot::new(
            inner_property,
            HashMap::from([(
                "some_segment_id_1".into(),
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
            )]),
        );

        // Both segment rules match. Expect the one with smaller order to be used:
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };
        let value = property.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }

    // No ordering to segment_rules is applied after the `Feature` object has been created.
    #[test]
    fn test_get_value_segment_rule_ordering() {
        // A helper function to return a value given some segment rules
        fn _get_value(segment_rules: Vec<TargetingRule>) -> Value {
            let inner_property = crate::models::Property {
                name: "F1".to_string(),
                property_id: "f1".to_string(),
                kind: ValueKind::Numeric,
                _format: None,
                value: ConfigValue(serde_json::Value::Number((-42).into())),
                segment_rules,
                _tags: None,
            };
            let property = PropertySnapshot::new(
                inner_property,
                HashMap::from([
                    (
                        "some_segment_id_1".into(),
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
                    ),
                    (
                        "some_segment_id_2".into(),
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
                    ),
                ]),
            );

            // Both segment rules match. Expect the one with smaller order to be used:
            let entity = crate::tests::GenericEntity {
                id: "a2".into(),
                attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
            };
            property.get_value(&entity).unwrap()
        }

        let mut segment_rules = vec![
            TargetingRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_1".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-48).into())),
                order: 1,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            },
            TargetingRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_2".into()],
                }],
                value: ConfigValue(serde_json::Value::Number((-49).into())),
                order: 0,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            },
        ];

        let value = _get_value(segment_rules.clone());
        assert!(matches!(value, Value::Int64(ref v) if v == &(-48)));

        segment_rules.sort_by(|a, b| a.order.cmp(&b.order));
        let value = _get_value(segment_rules);
        assert!(matches!(value, Value::Int64(ref v) if v == &(-49)));
    }
}

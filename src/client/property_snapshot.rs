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
            self.property.segment_rules.clone().into_iter(),
            entity,
        )? {
            Some(segment_rule) => {
                if segment_rule.value.is_default() {
                    Ok(self.property.value.clone())
                } else {
                    Ok(segment_rule.value)
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

    fn get_value_t<T: TryFrom<Value, Error = crate::Error>>(
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
            format: None,
            value: ConfigValue(serde_json::Value::Number((-42).into())),
            segment_rules: vec![TargetingRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_1".into()],
                }],
                value: ConfigValue(serde_json::Value::String("$default".into())),
                order: 1,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            }],
            tags: None,
        };
        let property = PropertySnapshot::new(
            inner_property,
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

    #[test]
    fn test_get_value_segment_rule_ordering() {
        let inner_property = crate::models::Property {
            name: "F1".to_string(),
            property_id: "f1".to_string(),
            kind: ValueKind::Numeric,
            format: None,
            value: ConfigValue(serde_json::Value::Number((-42).into())),
            segment_rules: vec![
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
            ],
            tags: None,
        };
        let property = PropertySnapshot::new(
            inner_property,
            HashMap::from([
                (
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
                ),
                (
                    "some_segment_id_2".into(),
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
                ),
            ]),
        );

        // Both segment rules match. Expect the one with smaller order to be used:
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };
        let value = property.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-49)));
    }

    // Test we can return the value as a primitive type
    #[test]
    fn test_get_value_t() {
        let inner_property = crate::models::Property {
            name: "F1".to_string(),
            property_id: "f1".to_string(),
            kind: ValueKind::Numeric,
            format: None,
            value: ConfigValue(serde_json::Value::Number((-42).into())),
            segment_rules: vec![TargetingRule {
                rules: vec![Segments {
                    segments: vec!["some_segment_id_1".into()],
                }],
                value: ConfigValue(serde_json::Value::String("$default".into())),
                order: 1,
                rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
            }],
            tags: None,
        };
        let property = PropertySnapshot::new(
            inner_property,
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
            )]),
        );

        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };

        // We fail to return it as f64, and also as u64
        let value: Result<f64> = property.get_value_t(&entity);
        assert!(matches!(value.unwrap_err(), crate::Error::MismatchType));
        let value: Result<u64> = property.get_value_t(&entity);
        assert!(matches!(value.unwrap_err(), crate::Error::MismatchType));

        // ...but we can return it as i64
        let value: i64 = property.get_value_t(&entity).unwrap();
        assert_eq!(value, -42);
    }
}

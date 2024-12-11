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

use crate::client::value::{NumericValue, Value};
use crate::entity::Entity;
use crate::Property;
use std::collections::HashMap;

use crate::errors::{Error, Result};
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
    fn get_id(&self) -> &str {
        &self.property.property_id
    }

    fn get_name(&self) -> Result<String> {
        Ok(self.property.name.clone())
    }

    fn get_data_type(&self) -> Result<crate::models::ValueKind> {
        Ok(self.property.kind)
    }

    fn get_value_default(&self) -> Result<crate::models::ConfigValue> {
        Ok(self.property.value.clone())
    }

    fn get_value(&self, entity: &impl Entity) -> Result<Value> {
        let model_value = self.evaluate_feature_for_entity(entity)?;

        let value = match self.property.kind {
            crate::models::ValueKind::Numeric => {
                Value::Numeric(NumericValue(model_value.0.clone()))
            }
            crate::models::ValueKind::Boolean => Value::Boolean(
                model_value
                    .0
                    .as_bool()
                    .ok_or(Error::ProtocolError("Expected Boolean".into()))?,
            ),
            crate::models::ValueKind::String => Value::String(
                model_value
                    .0
                    .as_str()
                    .ok_or(Error::ProtocolError("Expected String".into()))?
                    .to_string(),
            ),
        };
        Ok(value)
    }
}


#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        models::{ConfigValue, Segment, SegmentRule, Segments, TargetingRule, ValueKind},
        AttrValue,
    };

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
            attributes: HashMap::from([("name".into(), AttrValue::from("heinz".to_string()))]),
        };
        let value = property.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Numeric(ref v) if v.as_i64().unwrap() == -42));
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
            attributes: HashMap::from([("name".into(), AttrValue::from("heinz".to_string()))]),
        };
        let value = property.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Numeric(ref v) if v.as_i64().unwrap() == -49));
    }
}

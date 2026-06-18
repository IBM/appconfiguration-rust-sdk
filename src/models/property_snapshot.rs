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
use crate::{Property, PropertyEvaluationResult};

use crate::errors::Result;
use crate::models::evaluation_result::PropertyEvaluationDetails;
use crate::network::serialization::ValueType;
use crate::segment_evaluation::TargetingRules;

/// Provides a snapshot of a [`Property`].
#[derive(Debug)]
pub struct PropertySnapshot {
    value: Value,
    segment_rules: TargetingRules,
    value_type: ValueType,
    r#type: String,
    format: Option<String>,
    pub(crate) name: String,
    pub(crate) property_id: String,
    pub(crate) metering: Option<MeteringRecorderSender>,
}

impl PropertySnapshot {
    pub(crate) fn new(
        value: Value,
        segment_rules: TargetingRules,
        value_type: ValueType,
        r#type: String,
        format: Option<String>,
        name: &str,
        property_id: &str,
        metering: Option<MeteringRecorderSender>,
    ) -> Self {
        Self {
            value,
            segment_rules,
            value_type,
            r#type,
            format,
            name: name.to_string(),
            property_id: property_id.to_string(),
            metering,
        }
    }

    fn evaluate_property_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<(Value, PropertyEvaluationDetails)> {
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
                let segment_name = segment.map(|s| s.name.clone());
                let value = segment_rule.value(&self.value)?;
                Ok((
                    value,
                    PropertyEvaluationDetails {
                        value_type: "SEGMENT_VALUE".to_string(),
                        reason: format!(
                            "Matched targeting rule order {} for property evaluation.",
                            segment_rule.order()
                        ),
                        segment_name,
                    },
                ))
            }
            None => Ok((
                self.value.clone(),
                PropertyEvaluationDetails {
                    value_type: "DEFAULT_VALUE".to_string(),
                    reason: "No targeting rule matched. Returning property default value."
                        .to_string(),
                    segment_name: None,
                },
            )),
        }
    }

    pub fn is_secret_ref(&self) -> bool {
        matches!(self.value_type, ValueType::SecretRef)
    }
}

impl Property for PropertySnapshot {
    fn get_property_name(&self) -> Result<String> {
        Ok(self.name.clone())
    }

    fn get_property_id(&self) -> Result<String> {
        Ok(self.property_id.clone())
    }

    fn get_property_data_type(&self) -> Result<String> {
        Ok(self.r#type.clone())
    }

    fn get_property_data_format(&self) -> Result<Option<String>> {
        // If the Format is null or undefined for a String type, we default it to TEXT
        if self.format.is_none() && self.r#type == "STRING" {
            return Ok(Some("TEXT".to_string()));
        }
        Ok(self.format.clone())
    }

    fn get_current_value(&self, entity: &impl Entity) -> Result<PropertyEvaluationResult> {
        let (value, details) = self.evaluate_property_for_entity(entity)?;
        Ok(PropertyEvaluationResult { value, details })
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
    use crate::network::serialization::fixtures::create_one_segment_rule;
    use crate::network::serialization::{Rule, Segment, ValueType};
    use std::collections::HashMap;

    #[test]
    fn test_get_value_segment_with_default_value() {
        let property = {
            let segments = HashMap::from([(
                "some_segment_id_1".into(),
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
                "some_segment_id_1".into(),
                serde_json::Value::String("$default".into()),
                serde_json::Value::Number((100).into()),
            );
            let segment_rules =
                TargetingRules::new(segments, segment_rules, ValueType::Numeric, None);
            PropertySnapshot::new(
                Value::Int64(-42),
                segment_rules,
                ValueType::Numeric,
                "NUMERIC".to_string(),
                None,
                "F1",
                "f1",
                None,
            )
        };

        // Both segment rules match. Expect the one with smaller order to be used:
        let entity = crate::tests::GenericEntity {
            id: "a2".into(),
            attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
        };
        let value = property.get_current_value(&entity).unwrap();
        assert!(matches!(value.value, Value::Int64(ref v) if v == &(-42)));
    }
}

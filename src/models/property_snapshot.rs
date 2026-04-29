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
use crate::network::serialization::Segment;
use crate::segment_evaluation::{TargetingRule, TargetingRules};
use crate::value::Value;
use crate::network::serialization::ValueType;
use crate::{
    EvaluationContext, EvaluationRuleCondition, EvaluationRuleContext, EvaluationSegmentContext,
    Property, PropertyEvaluationResult,
};

/// Provides a snapshot of a [`Property`].
#[derive(Debug)]
pub struct PropertySnapshot {
    value: Value,
    segment_rules: TargetingRules,
    value_type: ValueType,
    pub(crate) name: String,
    pub(crate) property_id: String,
    pub(crate) metering: Option<MeteringRecorderSender>,
}

impl PropertySnapshot {
    pub(crate) fn new(
        value: Value,
        segment_rules: TargetingRules,
        value_type: ValueType,
        name: &str,
        property_id: &str,
        metering: Option<MeteringRecorderSender>,
    ) -> Self {
        Self {
            value,
            segment_rules,
            value_type,
            name: name.to_string(),
            property_id: property_id.to_string(),
            metering,
        }
    }

    pub fn is_secret_ref(&self) -> bool {
        matches!(self.value_type, ValueType::SecretRef)
    }

    fn evaluation_context(
        segment_rule: Option<&TargetingRule<'_>>,
        segment: Option<&Segment>,
    ) -> EvaluationContext {
        let matched_segment = segment.map(|segment| EvaluationSegmentContext {
            segment_id: segment.segment_id.clone(),
            name: segment.name.clone(),
            description: segment.description.clone(),
            tags: segment.tags.clone(),
            rules: segment
                .rules
                .iter()
                .map(|rule| EvaluationRuleCondition {
                    attribute_name: rule.attribute_name.clone(),
                    operator: rule.operator.clone(),
                    values: rule.values.clone(),
                })
                .collect(),
        });

        let matched_rule = segment_rule.map(|segment_rule| EvaluationRuleContext {
            order: segment_rule.order(),
            rollout_percentage: None,
            uses_default_value: segment_rule.uses_default_value(),
            targeted_segment_ids: segment_rule.targeted_segment_ids(),
        });

        EvaluationContext {
            matched_segment,
            matched_rule,
        }
    }

    fn evaluate_property_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<(Value, String, EvaluationContext)> {
        let (segment_rule, segment) = if self.segment_rules.is_empty() || entity.get_attributes().is_empty() {
            (None, None)
        } else {
            self.segment_rules
                .find_applicable_targeting_rule_and_segment_for_entity(entity)?
                .unzip()
        };

        self.record_evaluation(entity, segment);

        match segment_rule {
            Some(segment_rule) => {
                let context = Self::evaluation_context(Some(&segment_rule), segment);
                let value = segment_rule.value(&self.value)?;
                Ok((
                    value,
                    format!(
                        "Matched targeting rule order {} for property evaluation.",
                        segment_rule.order()
                    ),
                    context,
                ))
            }
            None => Ok((
                self.value.clone(),
                "No targeting rule matched. Returning property default value.".to_string(),
                EvaluationContext {
                    matched_segment: None,
                    matched_rule: None,
                },
            )),
        }
    }
}

impl Property for PropertySnapshot {
    fn get_name(&self) -> Result<String> {
        Ok(self.name.clone())
    }

    fn get_current_value(&self, entity: &impl Entity) -> Result<PropertyEvaluationResult> {
        let (value, details, context) = self.evaluate_property_for_entity(entity)?;
        Ok(PropertyEvaluationResult {
            value,
            details,
            context,
        })
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
            let segment_rules = TargetingRules::new(segments, segment_rules, ValueType::Numeric);
            PropertySnapshot::new(
                Value::Int64(-42),
                segment_rules,
                ValueType::Numeric,
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
        let value = property.get_value(&entity).unwrap();
        assert!(matches!(value, Value::Int64(ref v) if v == &(-42)));
    }
}

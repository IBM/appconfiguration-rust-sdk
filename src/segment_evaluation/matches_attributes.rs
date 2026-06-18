// (C) Copyright IBM Corp. 2025.
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

use std::collections::HashMap;

use super::errors::CheckOperatorErrorDetail;
use crate::Value;
use crate::network::serialization::{Rule, Segment};
use crate::segment_evaluation::errors::SegmentEvaluationError;
use crate::segment_evaluation::rule_operator::RuleOperator;

pub(crate) trait MatchesAttributes {
    type Error;

    fn matches_attributes(
        &self,
        attributes: &HashMap<String, Value>,
    ) -> std::result::Result<bool, Self::Error>;
}

impl MatchesAttributes for Segment {
    type Error = SegmentEvaluationError;

    /// A [`Segment`] matches attributes iif:
    /// * ALL the rules match the attributes
    fn matches_attributes(
        &self,
        attributes: &HashMap<String, Value>,
    ) -> std::result::Result<bool, Self::Error> {
        self.rules
            .iter()
            .map(|rule| {
                rule.matches_attributes(attributes)
                    .map_err(|(e, rule_value)| (e, self, rule, rule_value).into())
            })
            .collect::<std::result::Result<Vec<bool>, _>>()
            .map(|v| v.iter().all(|&x| x))
    }
}

impl MatchesAttributes for Rule {
    type Error = (CheckOperatorErrorDetail, String);

    /// A [`Rule`] matches attributes iif:
    /// * the attributes contain the requested attribute, AND
    /// * the attribute satisfies ANY of the rule values.
    ///
    /// TODO: What if rules.values is empty? Now it returns false
    fn matches_attributes(
        &self,
        attributes: &HashMap<String, Value>,
    ) -> std::result::Result<bool, Self::Error> {
        const NEGATION_OPERATORS: &[&str] =
            &["isNot", "notContains", "notStartsWith", "notEndsWith"];

        attributes
            .get(&self.attribute_name)
            .map_or(Ok(false), |attr_value| {
                let results = self
                    .values
                    .iter()
                    .map(|value| {
                        attr_value
                            .operate(&self.operator, value)
                            .map_err(|e| (e, value.to_owned()))
                    })
                    .collect::<std::result::Result<Vec<bool>, _>>()?;

                if NEGATION_OPERATORS.contains(&self.operator.as_str()) {
                    // ALL values must individually satisfy the negation
                    Ok(results.iter().all(|&x| x))
                } else {
                    // ANY single value match is sufficient
                    Ok(results.iter().any(|&x| x))
                }
            })
    }
}

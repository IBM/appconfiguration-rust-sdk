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

use thiserror::Error;

use crate::models::{Rule, Segment};

#[derive(Debug, Error)]
pub(crate) enum SegmentEvaluationError {
    #[error(transparent)]
    SegmentEvaluationFailed(#[from] SegmentEvaluationErrorKind),

    #[error("Segment ID '{0}' not found")]
    SegmentIdNotFound(String),
}

#[derive(Debug, Error)]
#[error(
    "Operation '{}' '{}' '{}' failed to evaluate: {}",
    segment_rule_attribute_name,
    segment_rule_operator,
    value,
    source
)]
pub(crate) struct SegmentEvaluationErrorKind {
    pub(crate) segment_id: String,
    pub(crate) segment_rule_attribute_name: String,
    pub(crate) segment_rule_operator: String,
    pub(crate) value: String,
    pub(crate) source: CheckOperatorErrorDetail,
}

impl From<(CheckOperatorErrorDetail, &Segment, &Rule, &String)> for SegmentEvaluationError {
    fn from(value: (CheckOperatorErrorDetail, &Segment, &Rule, &String)) -> Self {
        let (source, segment, segment_rule, value) = value;
        Self::SegmentEvaluationFailed(SegmentEvaluationErrorKind {
            segment_id: segment.segment_id.clone(),
            segment_rule_attribute_name: segment_rule.attribute_name.clone(),
            segment_rule_operator: segment_rule.operator.clone(),
            value: value.clone(),
            source,
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum CheckOperatorErrorDetail {
    #[error("Entity attribute is not a string.")]
    StringExpected,

    #[error("Entity attribute has unexpected type: Boolean.")]
    BooleanExpected(#[from] std::str::ParseBoolError),

    #[error("Entity attribute has unexpected type: float.")]
    FloatExpected(#[from] std::num::ParseFloatError),

    #[error("Entity attribute has unexpected type: integer.")]
    IntegerExpected(#[from] std::num::ParseIntError),

    #[error("Entity attribute is not a number.")]
    EntityAttrNotANumber,

    #[error("Operator not implemented.")]
    OperatorNotImplemented,
}

// Copyright 2026 IBM Corp. All Rights Reserved.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

//       http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationRuleContext {
    pub order: u32,
    pub rollout_percentage: Option<u32>,
    pub uses_default_value: bool,
    pub targeted_segment_ids: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationRuleCondition {
    pub attribute_name: String,
    pub operator: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationSegmentContext {
    pub segment_id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub rules: Vec<EvaluationRuleCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationContext {
    pub matched_segment: Option<EvaluationSegmentContext>,
    pub matched_rule: Option<EvaluationRuleContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureEvaluationDetails {
    #[serde(rename = "valueType")]
    pub value_type: String,
    pub reason: String,
    #[serde(rename = "segmentName", skip_serializing_if = "Option::is_none")]
    pub segment_name: Option<String>,
    #[serde(
        rename = "rolloutPercentageApplied",
        skip_serializing_if = "Option::is_none"
    )]
    pub rollout_percentage_applied: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyEvaluationDetails {
    #[serde(rename = "valueType")]
    pub value_type: String,
    pub reason: String,
    #[serde(rename = "segmentName", skip_serializing_if = "Option::is_none")]
    pub segment_name: Option<String>,
}

/// Returns: { value, isEnabled, details }
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEvaluationResult {
    pub value: Value,
    pub is_enabled: bool,
    pub details: FeatureEvaluationDetails,
}

/// Returns: { value, details }
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyEvaluationResult {
    pub value: Value,
    pub details: PropertyEvaluationDetails,
}

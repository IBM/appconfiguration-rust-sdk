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

use crate::Value;

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

#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEvaluationResult {
    pub value: Value,
    pub is_enabled: bool,
    pub details: String,
    pub context: EvaluationContext,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyEvaluationResult {
    pub value: Value,
    pub details: String,
    pub context: EvaluationContext,
}

// Made with Bob

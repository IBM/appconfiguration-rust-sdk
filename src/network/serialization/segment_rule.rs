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

use serde::Deserialize;

use crate::network::serialization::config_value::ConfigValue;
use crate::network::serialization::segments::Segments;

/// Associates a Feature/Property to one or more Segments
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct SegmentRule {
    /// The list of targeted segments
    /// NOTE: no rules by itself, but the rules are found in the segments
    /// NOTE: why list of lists?
    /// NOTE: why is this field called "rules"?
    pub rules: Vec<Segments>,
    pub value: ConfigValue,
    pub order: u32,
    pub rollout_percentage: Option<ConfigValue>,
}

#[cfg(test)]
pub mod fixtures {
    use super::*;
    use crate::network::serialization::SegmentRule;
    use rstest::*;

    pub fn create_one_segment_rule(
        segment_id: String,
        value: serde_json::Value,
        rollout_percentage: serde_json::Value,
    ) -> Vec<SegmentRule> {
        vec![SegmentRule {
            rules: vec![Segments {
                segments: vec![segment_id],
            }],
            value: ConfigValue(value),
            order: 0,
            rollout_percentage: Some(ConfigValue(rollout_percentage)),
        }]
    }

    #[fixture]
    pub fn one_segment_rule() -> Vec<SegmentRule> {
        vec![SegmentRule {
            rules: vec![Segments {
                segments: vec!["some_segment_id".into()],
            }],
            value: ConfigValue(serde_json::Value::Number((-48).into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number((50).into()))),
        }]
    }

    #[fixture]
    pub fn some_segment_rules() -> Vec<SegmentRule> {
        vec![SegmentRule {
            rules: vec![
                Segments {
                    segments: vec!["some_segment_id_1".into(), "some_segment_id_2".into()],
                },
                Segments {
                    segments: vec!["some_segment_id_3".into()],
                },
            ],
            value: ConfigValue(serde_json::Value::Number((-48).into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
        }]
    }

    #[fixture]
    pub fn segment_rules_with_invalid_segment_id() -> Vec<SegmentRule> {
        vec![SegmentRule {
            rules: vec![Segments {
                segments: vec!["non_existing_segment_id".into()],
            }],
            value: ConfigValue(serde_json::Value::Number((-48).into())),
            order: 0,
            rollout_percentage: Some(ConfigValue(serde_json::Value::Number((100).into()))),
        }]
    }
}

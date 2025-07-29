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

use super::{SegmentRule, ValueType};
use crate::network::serialization::config_value::ConfigValue;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Feature {
    pub name: String,
    pub feature_id: String,
    pub r#type: ValueType,
    pub format: Option<String>,
    pub enabled_value: ConfigValue,
    pub disabled_value: ConfigValue,
    pub segment_rules: Vec<SegmentRule>,
    pub enabled: bool,
    pub rollout_percentage: u32,
}

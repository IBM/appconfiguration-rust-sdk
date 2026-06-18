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

use serde::{Deserialize, Serialize};

/// Represents a single phase in a progressive rollout
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct RolloutPhase {
    /// The rollout percentage for this phase (0-100)
    pub percentage: u32,
    /// Duration for this phase
    pub duration: Option<u32>,
    /// Duration type: "minutes", "hours", or "days"
    pub duration_type: Option<String>,
}

/// Configuration for progressive rollout
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct RolloutConfiguration {
    /// ISO 8601 timestamp when the rollout starts
    pub start_at: String,
    /// List of rollout phases
    pub phases: Vec<RolloutPhase>,
}

/// Rollout type constants — used for future experiment / progressive-rollout checks

pub const ROLLOUT_TYPE_PROGRESSIVE: &str = "PROGRESSIVE";

/// Delimiter used to combine feature_id and rule_id
pub const DELIMITER: char = '\u{001F}'; // Unit Separator character

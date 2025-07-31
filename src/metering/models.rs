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

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MeteringDataUsageJson {
    pub feature_id: Option<String>,
    pub property_id: Option<String>,
    pub entity_id: String,
    // Serialized as "nil" when None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_id: Option<String>,
    // When this evaluation was last done
    pub evaluation_time: DateTime<Utc>,
    // how often this was evaluated
    pub count: u32,
}

/// Represents Metering data in a structure for data exchange used for
/// sending to the server.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MeteringDataJson {
    pub collection_id: String,
    pub environment_id: String,
    pub usages: Vec<MeteringDataUsageJson>,
}

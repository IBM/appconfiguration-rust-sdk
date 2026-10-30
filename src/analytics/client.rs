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

use crate::analytics::errors::AnalyticsResult;
use crate::analytics::record::AnalyticsPayload;

/// Abstraction over the HTTP transport — allows mocking in tests.
pub(crate) trait AnalyticsClient: Send + 'static {
    /// POST `payload` to the analytics endpoint for the given `guid`.
    fn push_analytics_data(&self, guid: &str, payload: &AnalyticsPayload) -> AnalyticsResult<()>;
}

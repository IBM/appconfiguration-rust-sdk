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

use std::fmt::Display;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ConfigValue(pub(crate) serde_json::Value);

impl ConfigValue {
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.0.as_u64()
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    pub fn as_boolean(&self) -> Option<bool> {
        self.0.as_bool()
    }

    pub fn as_string(&self) -> Option<String> {
        self.0.as_str().map(|s| s.to_string())
    }

    pub fn is_default(&self) -> bool {
        if let Some(s) = self.0.as_str() {
            s == "$default"
        } else {
            false
        }
    }
}

impl Display for ConfigValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

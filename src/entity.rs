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

use std::collections::HashMap;

use crate::Value;

/// An object on which evaluate properties and features.
pub trait Entity {
    /// Gets a unique identifier for the entity.
    fn get_id(&self) -> String;

    /// Gets a map of attributes names and values against which evaluate the
    /// entities belonging to segments.
    fn get_attributes(&self) -> HashMap<String, Value> {
        HashMap::new()
    }
}

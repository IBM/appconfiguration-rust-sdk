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

use crate::client::value::Value;
use crate::errors::Result;
use crate::Entity;

pub trait Property {
    fn get_id(&self) -> &str;

    fn get_name(&self) -> Result<String>;

    fn get_data_type(&self) -> Result<crate::models::ValueKind>;

    fn get_value_default(&self) -> Result<crate::models::ConfigValue>;

    fn get_value(&self, entity: &impl Entity) -> Result<Value>;
}

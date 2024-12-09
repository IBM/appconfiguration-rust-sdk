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

use crate::client::value::{NumericValue, Value};
use crate::errors::{Error, Result};
use crate::models::ValueKind;
use crate::Entity;

pub trait Resource {
    fn get_value(&self, entity: &impl Entity) -> Result<Value> {
        let model_value = self.evaluate_feature_for_entity(entity)?;

        let value = match self.value_type() {
            crate::models::ValueKind::Numeric => {
                Value::Numeric(NumericValue(model_value.0.clone()))
            }
            crate::models::ValueKind::Boolean => {
                Value::Boolean(model_value.0.as_bool().ok_or(Error::ProtocolError)?)
            }
            crate::models::ValueKind::String => Value::String(
                model_value
                    .0
                    .as_str()
                    .ok_or(Error::ProtocolError)?
                    .to_string(),
            ),
        };
        Ok(value)
    }

    fn value_type(&self) -> &ValueKind;

    fn evaluate_feature_for_entity(
        &self,
        entity: &impl Entity,
    ) -> Result<crate::models::ConfigValue>;
}

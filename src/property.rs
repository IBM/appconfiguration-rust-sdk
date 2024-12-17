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

use crate::errors::Result;
use crate::{Entity, Value};

/// Access to data and evaluation of IBM AppConfiguration properties
pub trait Property {
    /// Returns the full name of the property.
    fn get_name(&self) -> Result<String>;

    /// Evaluates a property for the given [`Entity`] and returns a [`Value`].
    /// 
    /// Use the methods available in [`Value`] to return the actual primitive value. If
    /// all you want is the primitive value, you can use the method
    /// [`get_value_into`](Property::get_value_into) instead.
    /// 
    /// # Examples
    ///
    /// ```
    /// # use appconfiguration::{AppConfigurationClient, Property, Result, Entity, Value};
    /// # fn doctest_get_value(client: impl AppConfigurationClient, entity: &impl Entity) -> Result<()> {
    ///     let property = client.get_property("my_property")?;
    ///     let value: Value = property.get_value(entity)?;
    /// 
    ///     match value {
    ///         Value::Float64(v) => println!("f64 with value {v}"),
    ///         Value::UInt64(v) => println!("u64 with value {v}"),
    ///         Value::Int64(v) => println!("i64 with value {v}"),
    ///         Value::String(v) => println!("String with value {v}"),
    ///         Value::Boolean(v) => println!("bool with value {v}"),
    ///     }
    /// #   Ok(())
    /// # }
    /// ```
    fn get_value(&self, entity: &impl Entity) -> Result<Value>;

    /// Evaluates a property for the given [`Entity`] and returns its value converted (if possible)
    /// to the given type.
    /// 
    /// # Examples
    ///
    /// ```
    /// # use appconfiguration::{AppConfigurationClient, Property, Result, Entity};
    /// # fn doctest_get_value_into(client: impl AppConfigurationClient, entity: &impl Entity) -> Result<()> {
    ///     let property = client.get_property("my_bool_feature")?;
    ///     let value: bool = property.get_value_into(entity)?;
    /// 
    ///     // an bool cannot be returned as something else
    ///     assert!(property.get_value_into::<f64>(entity).is_err());
    ///     assert!(property.get_value_into::<String>(entity).is_err());
    ///     assert!(property.get_value_into::<i64>(entity).is_err());
    /// #   Ok(())
    /// # }
    /// ```
    fn get_value_into<T: TryFrom<Value, Error = crate::Error>>(
        &self,
        entity: &impl Entity,
    ) -> Result<T>;
}

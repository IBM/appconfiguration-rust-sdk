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

/// Access to data and evaluation of IBM AppConfiguration features
pub trait Feature {
    /// Returns the full name of the feature
    fn get_name(&self) -> Result<String>;

    /// Returns if the feature is enabled or not.
    /// 
    /// An enabled feature will be evaluated for each [`Entity`] to return the 
    /// corresponding value. However, disabled features, won't be evaluated and
    /// will always return the disabled value.
    fn is_enabled(&self) -> Result<bool>;

    /// Returns the evaluated value as a [`Value`] instance
    /// 
    /// # Examples
    ///
    /// ```
    /// # use appconfiguration_rust_sdk::{AppConfigurationClient, Feature, Result, Entity, Value};
    /// # fn doctest_get_value(client: AppConfigurationClient, entity: &impl Entity) -> Result<()> {
    ///     let feature = client.get_feature("my_feature")?;
    ///     let value: Value = feature.get_value(entity)?;
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

    /// Returns the evaluated value as the given primitive type, if possible
    /// 
    /// # Examples
    ///
    /// ```
    /// # use appconfiguration_rust_sdk::{AppConfigurationClient, Feature, Result, Entity};
    /// # fn doctest_get_value_t(client: AppConfigurationClient, entity: &impl Entity) -> Result<()> {
    ///     let feature = client.get_feature("my_f64_feature")?;
    ///     let value: f64 = feature.get_value_t(entity)?;
    /// 
    ///     // an f64 cannot be returned as u64
    ///     let failed: Result<u64> = feature.get_value_t(entity);
    ///     assert!(failed.is_err());
    /// #   Ok(())
    /// # }
    /// ```
    fn get_value_t<T: TryFrom<Value, Error = crate::Error>>(
        &self,
        entity: &impl Entity,
    ) -> Result<T>;
}

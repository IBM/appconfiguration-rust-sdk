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

//! The IBM Cloud App Configuration Rust SDK is used to perform feature flag and property
//! evaluation based on the configuration on IBM Cloud App Configuration service.
//!
//! # Overview
//!
//! [IBM Cloud App Configuration](https://cloud.ibm.com/docs/app-configuration) is a centralized
//! feature management and configuration service on [IBM Cloud](https://www.cloud.ibm.com) for
//! use with web and mobile applications, microservices, and distributed environments.
//!
//! Instrument your applications with App Configuration Rust SDK, and use the App Configuration
//! dashboard, API or CLI to define feature flags or properties, organized into collections and
//! targeted to segments. Change feature flag states in the cloud to activate or deactivate features
//! in your application or environment, when required. You can also manage the properties for distributed
//! applications centrally.
//!
//! # Pre-requisites
//!
//! You will need the `apikey`, `region` and `guid` for the AppConfiguration you want to connect to
//! from your [IBMCloud account](https://cloud.ibm.com/).
//!
//! # Usage
//!
//! **Note.-** This crate is still under heavy development. Breaking changes are expected.
//!
//! ## Recommended top-level SDK flow
//!
//! The Rust SDK now exposes a Node-style top-level wrapper through
//! [`AppConfigurationSdk`](appconfiguration-rust-sdk/src/client/app_configuration_sdk.rs), mirroring
//! the high-level `init()` + `setContext()` flow from the Node SDK.
//!
//! ```
//! use appconfiguration::{
//!     AppConfigurationContextOptions, AppConfigurationSdk, ConfigurationProvider, Entity, Feature,
//!     Result, Value,
//! };
//! # use std::collections::HashMap;
//! # pub struct MyEntity;
//! # impl Entity for MyEntity {
//! #   fn get_id(&self) -> String {
//! #     "TrivialId".into()
//! #   }
//! #   fn get_attributes(&self) -> HashMap<String, Value> {
//! #     HashMap::new()
//! #   }
//! # }
//! # fn func() -> Result<()> {
//! # let apikey: &str = "api_key";
//! # let region: &str = "us-south";
//! # let guid: &str = "12345678-1234-1234-1234-12345678abcd";
//! # let environment_id: &str = "production";
//! # let collection_id: &str = "ecommerce";
//! let mut sdk = AppConfigurationSdk::new();
//! sdk.use_private_endpoint(false);
//! sdk.init(region, guid, apikey)?;
//! sdk.set_context(
//!     collection_id,
//!     environment_id,
//!     AppConfigurationContextOptions::default(),
//! )?;
//!
//! let feature = sdk.get_feature("AB_testing_feature")?;
//! let user = MyEntity;
//! let value_for_this_user = feature.get_value(&user)?.try_into()?;
//! if value_for_this_user {
//!     println!("Feature {} is active for user {}", feature.get_name()?, user.get_id());
//! } else {
//!     println!("User {} keeps using the legacy workflow", user.get_id());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Lower-level direct client flow
//!
//! The lower-level [`AppConfigurationClientIBMCloud`](appconfiguration-rust-sdk/src/client/app_configuration_ibm_cloud.rs)
//! constructor remains available for callers that want to build directly from
//! [`ConfigurationId`](appconfiguration-rust-sdk/src/client/app_configuration_client.rs).
//!
//! ```
//! use appconfiguration::{
//!     AppConfigurationClientIBMCloud, ConfigurationId, ConfigurationProvider, Entity, Feature,
//!     OfflineMode, Result, Value,
//! };
//! # use std::collections::HashMap;
//! # pub struct MyEntity;
//! # impl Entity for MyEntity {
//! #   fn get_id(&self) -> String {
//! #     "TrivialId".into()
//! #   }
//! #   fn get_attributes(&self) -> HashMap<String, Value> {
//! #     HashMap::new()
//! #   }
//! # }
//! # fn func() -> Result<()> {
//! # let apikey: &str = "api_key";
//! # let region: &str = "us-south";
//! # let guid: String = "12345678-1234-1234-1234-12345678abcd".to_string();
//! # let environment_id: String = "production".to_string();
//! # let collection_id: String = "ecommerce".to_string();
//! let configuration = ConfigurationId::new(guid, environment_id, collection_id);
//! let client = AppConfigurationClientIBMCloud::new(
//!     &apikey,
//!     &region,
//!     configuration,
//!     OfflineMode::Fail,
//!     false,
//! )?;
//!
//! let feature = client.get_feature("AB_testing_feature")?;
//! let user = MyEntity;
//! let value_for_this_user = feature.get_value(&user)?.try_into()?;
//! if value_for_this_user {
//!     println!("Feature {} is active for user {}", feature.get_name()?, user.get_id());
//! } else {
//!     println!("User {} keeps using the legacy workflow", user.get_id());
//! }
//! # Ok(())
//! # }
//! ```
//!
mod client;
mod entity;
mod errors;
mod feature;
pub(crate) mod metering;
mod models;
mod network;
mod property;
mod segment_evaluation;
pub(crate) mod utils;
mod value;

pub use client::{
    AppConfigurationClient, AppConfigurationClientIBMCloud, AppConfigurationContextOptions,
    AppConfigurationOffline, AppConfigurationSdk, ConfigurationId, ConfigurationProvider,
    RuntimeEventEmitter,
};
pub use entity::Entity;
pub use errors::{ConfigurationDataError, Error, Result};
pub use feature::Feature;
pub use models::{
    EvaluationContext, EvaluationRuleCondition, EvaluationRuleContext, EvaluationSegmentContext,
    FeatureEvaluationResult, PropertyEvaluationResult, SecretManager, SecretPropertySnapshot,
};
pub use network::live_configuration::OfflineMode;
pub(crate) use network::{ServerClientImpl, TokenProviderImpl};
pub use property::Property;
pub use value::Value;

pub use network::ServiceAddress;
#[cfg(test)]
mod tests;

#[cfg(feature = "test_utils")]
pub mod test_utils;

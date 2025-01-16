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
//! Create your client with the context (environment and collection) you want to connect to
//!
//! ```
//! use appconfiguration::{
//!     AppConfigurationClient, AppConfigurationClientIBMCloud,
//!     Entity, Result, Value, Feature
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
//!
//! // Create the client connecting to the server
//! let client = AppConfigurationClientIBMCloud::new(&apikey, &region, &guid, &environment_id, &collection_id)?;
//!
//! // Get the feature you want to evaluate for your entities
//! let feature = client.get_feature("AB_testing_feature")?;
//!
//! // Evaluate feature value for each one of your entities
//! let user = MyEntity; // Implements Entity
//!
//! let value_for_this_user = feature.get_value(&user)?.try_into()?;
//! if value_for_this_user {
//!     println!("Feature {} is active for user {}", feature.get_name()?, user.get_id());
//! } else {
//!     println!("User {} keeps using the legacy workflow", user.get_id());
//! }
//!
//! # Ok(())
//! # }
//! ```
//!
mod client;
mod entity;
mod errors;
mod feature;
mod models;
mod network;
mod property;
mod segment_evaluation;
mod value;

pub use client::{AppConfigurationClient, AppConfigurationClientIBMCloud, AppConfigurationOffline};
pub use entity::Entity;
pub use errors::{Error, Result};
pub use feature::Feature;
pub use network::TokenProvider;
pub(crate) use network::{IBMCloudTokenProvider, ServerClientImpl};
pub use property::Property;
pub use value::Value;

#[cfg(test)]
mod tests;

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

mod app_configuration_client;
mod app_configuration_http;
mod app_configuration_ibm_cloud;
mod app_configuration_offline;

pub(crate) mod configuration;
pub(crate) mod feature_proxy;
pub(crate) mod feature_snapshot;
pub(crate) mod metering;
pub(crate) mod property_proxy;
pub(crate) mod property_snapshot;

pub use app_configuration_client::{
    AppConfigurationClient, ConfigurationId, ConfigurationProvider,
};

pub(crate) use app_configuration_http::AppConfigurationClientHttp;
pub use app_configuration_ibm_cloud::AppConfigurationClientIBMCloud;
pub use app_configuration_offline::AppConfigurationOffline;

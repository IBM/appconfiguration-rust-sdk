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

use crate::{ConfigurationProvider, Result};

use crate::client::feature_proxy::FeatureProxy;

use crate::client::property_proxy::PropertyProxy;

/// Identifies a configuration
#[derive(Debug, Clone)]
pub struct ConfigurationId {
    /// Instance ID of the App Configuration service. Obtain it from the service credentials section of the App Configuration dashboard
    pub guid: String,
    /// ID of the environment created in App Configuration service instance under the Environments section.
    pub environment_id: String,
    /// ID of the collection created in App Configuration service instance under the Collections section
    pub collection_id: String,
}

impl ConfigurationId {
    pub fn new(guid: String, environment_id: String, collection_id: String) -> Self {
        Self {
            guid,
            environment_id,
            collection_id,
        }
    }
}

/// AppConfiguration client for browsing, and evaluating features and properties.
pub trait AppConfigurationClient: ConfigurationProvider {
    /// Returns a proxied [`Feature`](crate::Feature).
    ///
    /// This proxied feature will envaluate entities using the latest information
    /// available if the client implementation support some kind of live-updates.
    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>>;

    /// Returns a proxied [`Property`](crate::Property).
    ///
    /// This proxied property will envaluate entities using the latest information
    /// available if the client implementation support some kind of live-updates.
    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy>;
}

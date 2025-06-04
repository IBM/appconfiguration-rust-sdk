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

use crate::client::feature_snapshot::FeatureSnapshot;
use crate::client::property_snapshot::PropertySnapshot;
use crate::errors::Result;
use crate::network::live_configuration::LiveConfigurationImpl;
use crate::network::ServiceAddress;
use crate::{ConfigurationProvider, IBMCloudTokenProvider, OfflineMode};

use super::ConfigurationId;
use crate::client::app_configuration_http::AppConfigurationClientHttp;

/// AppConfiguration client connection to IBM Cloud.
#[derive(Debug)]
pub struct AppConfigurationClientIBMCloud {
    client: AppConfigurationClientHttp<LiveConfigurationImpl>,
}

impl AppConfigurationClientIBMCloud {
    /// Creates a new [`AppConfigurationClient`] connecting to IBM Cloud.
    ///
    /// This client keeps a websocket open to the server to receive live-updates
    /// to features and properties.
    ///
    /// # Arguments
    ///
    /// * `apikey` - The encrypted API key.
    /// * `region` - Region name where the App Configuration service instance is created
    /// * `configuration_id` - Identifies the App Configuration configuration to use.
    /// * `offline_mode` - Behavior when the configuration might not be synced with the server
    pub fn new(
        apikey: &str,
        region: &str,
        configuration_id: ConfigurationId,
        offline_mode: OfflineMode,
    ) -> Result<Self> {
        let service_address = Self::create_service_address(region);
        let token_provider = Box::new(IBMCloudTokenProvider::new(apikey));
        Ok(Self {
            client: AppConfigurationClientHttp::new(
                service_address,
                token_provider,
                configuration_id,
                offline_mode,
            )?,
        })
    }

    fn create_service_address(region: &str) -> ServiceAddress {
        ServiceAddress::new(
            format!("{region}.apprapp.cloud.ibm.com"),
            None,
            Some("apprapp".to_string()),
        )
    }
}

impl ConfigurationProvider for AppConfigurationClientIBMCloud {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        self.client.get_feature_ids()
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        self.client.get_feature(feature_id)
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        self.client.get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        self.client.get_property(property_id)
    }

    fn is_online(&self) -> Result<bool> {
        self.client.is_online()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::network::http_client::ServiceAddressProtocol;

    #[test]
    fn test_ibm_service_address() {
        let service_address = AppConfigurationClientIBMCloud::create_service_address("region");

        assert_eq!(
            service_address.base_url(ServiceAddressProtocol::Http),
            "https://region.apprapp.cloud.ibm.com/apprapp"
        );
        assert_eq!(
            service_address.base_url(ServiceAddressProtocol::Ws),
            "wss://region.apprapp.cloud.ibm.com/apprapp"
        );
    }
}

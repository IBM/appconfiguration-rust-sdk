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

pub use crate::client::feature_proxy::FeatureProxy;
use crate::client::feature_snapshot::FeatureSnapshot;
pub use crate::client::property_proxy::PropertyProxy;
use crate::client::property_snapshot::PropertySnapshot;
use crate::errors::Result;

use crate::network::live_configuration::{CurrentMode, LiveConfiguration, LiveConfigurationImpl};
use crate::network::{ServiceAddress, TokenProvider};
use crate::ServerClientImpl;

use super::{AppConfigurationClient, ConfigurationId};

/// AppConfiguration client implementation that connects to a server
#[derive(Debug)]
pub struct AppConfigurationClientHttp<T: LiveConfiguration> {
    live_configuration: T,
}

impl AppConfigurationClientHttp<LiveConfigurationImpl> {
    /// Creates a new [`AppConfigurationClient`] connecting to the server specified in the constructor arguments
    ///
    /// This client keeps a websocket open to the server to receive live-updates
    /// to features and properties.
    ///
    /// # Arguments
    ///
    /// * `service_address` - The address of the server to connect to.
    /// * `token_provider` - An object that can provide the tokens required by the server.
    /// * `configuration_id` - Identifies the App Configuration configuration to use.
    pub fn new(
        service_address: ServiceAddress,
        token_provider: Box<dyn TokenProvider>,
        configuration_id: ConfigurationId,
    ) -> Result<Self> {
        let server_client = ServerClientImpl::new(service_address, token_provider)?;

        let live_configuration = LiveConfigurationImpl::new(server_client, configuration_id);
        Ok(Self { live_configuration })
    }
}

impl<T: LiveConfiguration> AppConfigurationClientHttp<T> {
    pub fn is_online(&self) -> Result<bool> {
        Ok(self.live_configuration.get_current_mode()? == CurrentMode::Online)
    }
}

impl<T: LiveConfiguration> AppConfigurationClient for AppConfigurationClientHttp<T> {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .live_configuration
            .get_configuration()?
            .get_feature_ids()
            .into_iter()
            .cloned()
            .collect())
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        self.live_configuration
            .get_configuration()?
            .get_feature(feature_id)
    }

    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>> {
        // FIXME: there is and was no validation happening if the feature exists.
        // Comments and error messages in FeatureProxy suggest that this should happen here.
        // same applies for properties.
        Ok(FeatureProxy::new(self, feature_id.to_string()))
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .live_configuration
            .get_configuration()?
            .get_property_ids()
            .into_iter()
            .cloned()
            .collect())
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        self.live_configuration
            .get_configuration()?
            .get_property(property_id)
    }

    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy> {
        Ok(PropertyProxy::new(self, property_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::configuration::Configuration;
    use crate::models::tests::{
        configuration_feature1_enabled, configuration_property1_enabled,
        example_configuration_enterprise,
    };
    use crate::utils::ThreadStatus;
    use crate::{models::ConfigurationJson, Feature, Property};
    use rstest::rstest;

    struct LiveConfigurationMock {
        configuration: Configuration,
    }
    impl LiveConfiguration for LiveConfigurationMock {
        fn get_configuration(&self) -> crate::network::live_configuration::Result<Configuration> {
            Ok(self.configuration.clone())
        }

        fn get_thread_status(
            &mut self,
        ) -> ThreadStatus<crate::network::live_configuration::Result<()>> {
            todo!()
        }

        fn get_current_mode(&self) -> crate::network::live_configuration::Result<CurrentMode> {
            todo!()
        }
    }

    #[rstest]
    fn test_get_feature_persistence(
        example_configuration_enterprise: ConfigurationJson,
        configuration_feature1_enabled: ConfigurationJson,
    ) {
        let mut client = {
            let configuration_snapshot =
                Configuration::new("dev", example_configuration_enterprise).unwrap();

            let live_cfg_mock = LiveConfigurationMock {
                configuration: configuration_snapshot,
            };

            AppConfigurationClientHttp {
                live_configuration: live_cfg_mock,
            }
        };

        let feature = client.get_feature("f1").unwrap();

        let entity = crate::entity::tests::TrivialEntity {};
        let feature_value1 = feature.get_value(&entity).unwrap();

        // We simulate an update of the configuration:
        let configuration_snapshot =
            Configuration::new("environment_id", configuration_feature1_enabled).unwrap();
        client.live_configuration = LiveConfigurationMock {
            configuration: configuration_snapshot,
        };
        // The feature value should not have changed (as we did not retrieve it again)
        let feature_value2 = feature.get_value(&entity).unwrap();
        assert_eq!(feature_value2, feature_value1);

        // Now we retrieve the feature again:
        let feature = client.get_feature("f1").unwrap();
        // And expect the updated value
        let feature_value3 = feature.get_value(&entity).unwrap();
        assert_ne!(feature_value3, feature_value1);
    }

    #[rstest]
    fn test_get_property_persistence(
        example_configuration_enterprise: ConfigurationJson,
        configuration_property1_enabled: ConfigurationJson,
    ) {
        let mut client = {
            let configuration_snapshot =
                Configuration::new("dev", example_configuration_enterprise).unwrap();

            let live_cfg_mock = LiveConfigurationMock {
                configuration: configuration_snapshot,
            };

            AppConfigurationClientHttp {
                live_configuration: live_cfg_mock,
            }
        };

        let property = client.get_property("p1").unwrap();

        let entity = crate::entity::tests::TrivialEntity {};
        let property_value1 = property.get_value(&entity).unwrap();

        // We simulate an update of the configuration:
        let configuration_snapshot =
            Configuration::new("environment_id", configuration_property1_enabled).unwrap();
        client.live_configuration = LiveConfigurationMock {
            configuration: configuration_snapshot,
        };
        // The property value should not have changed (as we did not retrieve it again)
        let property_value2 = property.get_value(&entity).unwrap();
        assert_eq!(property_value2, property_value1);

        // Now we retrieve the property again:
        let property = client.get_property("p1").unwrap();
        // And expect the updated value
        let property_value3 = property.get_value(&entity).unwrap();
        assert_ne!(property_value3, property_value1);
    }
}

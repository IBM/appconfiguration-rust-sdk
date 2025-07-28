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

use crate::metering::{start_metering, MeteringClient, MeteringRecorder};
use crate::network::live_configuration::{LiveConfiguration, LiveConfigurationImpl};
use crate::network::{ServiceAddress, TokenProvider};
use crate::{ConfigurationProvider, OfflineMode, ServerClientImpl};

use super::ConfigurationId;

/// AppConfiguration client implementation that connects to a server
#[derive(Debug)]
pub(crate) struct AppConfigurationClientHttp<T: LiveConfiguration> {
    live_configuration: T,
    metering: MeteringRecorder,
}

impl AppConfigurationClientHttp<LiveConfigurationImpl> {
    /// Creates a new [`crate::AppConfigurationClient`] connecting to the server specified in the constructor arguments
    ///
    /// This client keeps a websocket open to the server to receive live-updates
    /// to features and properties.
    ///
    /// # Arguments
    ///
    /// * `service_address` - The address of the server to connect to.
    /// * `token_provider` - An object that can provide the tokens required by the server.
    /// * `configuration_id` - Identifies the App Configuration configuration to use.
    /// * `offline_mode` - Behavior when the configuration might not be synced with the server
    pub fn new(
        service_address: ServiceAddress,
        token_provider: Box<dyn TokenProvider>,
        configuration_id: ConfigurationId,
        offline_mode: OfflineMode,
    ) -> Result<Self> {
        let server_client = ServerClientImpl::new(service_address, token_provider)?;
        let metering_client = MeteringClientImpl;

        let metering = start_metering(
            configuration_id.clone(),
            std::time::Duration::from_secs(10 * 60),
            metering_client,
        );

        let live_configuration =
            LiveConfigurationImpl::new(offline_mode, server_client, configuration_id);
        Ok(Self {
            live_configuration,
            metering,
        })
    }
}

impl<T: LiveConfiguration> ConfigurationProvider for AppConfigurationClientHttp<T> {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        self.live_configuration.get_feature_ids()
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        let mut feature = self.live_configuration.get_feature(feature_id)?;
        feature.metering = Some(self.metering.sender.clone());
        Ok(feature)
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        self.live_configuration.get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        let mut property = self.live_configuration.get_property(property_id)?;
        property.metering = Some(self.metering.sender.clone());
        Ok(property)
    }

    fn is_online(&self) -> Result<bool> {
        self.live_configuration.is_online()
    }
}

struct MeteringClientImpl;

impl MeteringClient for MeteringClientImpl {
    fn push_metering_data(
        &self,
        _data: &crate::models::MeteringDataJson,
    ) -> crate::metering::MeteringResult<()> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::configuration::Configuration;
    use crate::metering::metering::tests::start_metering_mock;
    use crate::models::tests::{
        configuration_feature1_enabled, configuration_property1_enabled,
        example_configuration_enterprise,
    };
    use crate::network::live_configuration::CurrentMode;
    use crate::utils::ThreadStatus;
    use crate::{Feature, Property};
    use rstest::rstest;

    struct LiveConfigurationMock {
        configuration: Configuration,
    }
    impl ConfigurationProvider for LiveConfigurationMock {
        fn get_feature_ids(&self) -> Result<Vec<String>> {
            self.configuration.get_feature_ids()
        }

        fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
            self.configuration.get_feature(feature_id)
        }

        fn get_property_ids(&self) -> Result<Vec<String>> {
            self.configuration.get_property_ids()
        }

        fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
            self.configuration.get_property(property_id)
        }

        fn is_online(&self) -> Result<bool> {
            todo!()
        }
    }
    impl LiveConfiguration for LiveConfigurationMock {
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
        example_configuration_enterprise: Configuration,
        configuration_feature1_enabled: Configuration,
    ) {
        let (mut client, metering_recv) = {
            let live_cfg_mock = LiveConfigurationMock {
                configuration: example_configuration_enterprise,
            };

            let configuration_id = ConfigurationId::new(
                "test_guid".to_string(),
                "test_env_id".to_string(),
                "test_collection_id".to_string(),
            );
            let (metering, metering_recv) = start_metering_mock(configuration_id);

            (
                AppConfigurationClientHttp {
                    live_configuration: live_cfg_mock,
                    metering,
                },
                metering_recv,
            )
        };

        let feature = client.get_feature("f1").unwrap();

        let entity = crate::entity::tests::TrivialEntity {};
        let feature_value1 = feature.get_value(&entity).unwrap();

        // We simulate an update of the configuration:
        client.live_configuration = LiveConfigurationMock {
            configuration: configuration_feature1_enabled,
        };
        // The feature value should not have changed (as we did not retrieve it again)
        let feature_value2 = feature.get_value(&entity).unwrap();
        assert_eq!(feature_value2, feature_value1);

        // Now we retrieve the feature again:
        let feature = client.get_feature("f1").unwrap();
        // And expect the updated value
        let feature_value3 = feature.get_value(&entity).unwrap();
        assert_ne!(feature_value3, feature_value1);

        // We evaluated the property 3 times (for two different configurations)
        {
            let metering_data = metering_recv.recv().unwrap();
            // The value for the `collection_id` and `environment_id` comes from the `ConfigurationId`
            // object that was provided to the `start_metering` function. It doesn't match
            // the `ConfigurationId` that was used to get the `Configuration` object. This
            // inconsistency is only reachable in these tests, not via the public API, so
            // there is nothing to fix right now.
            assert_eq!(metering_data.collection_id, "test_collection_id");
            assert_eq!(metering_data.environment_id, "test_env_id");

            // We expect 3 evaluations to be covered in metering.
            // Do not care about the way they are sorted.
            let total_counts: u32 = metering_data.usages.iter().map(|usage| usage.count).sum();
            assert_eq!(total_counts, 3);
        }
    }

    #[rstest]
    fn test_get_property_persistence(
        example_configuration_enterprise: Configuration,
        configuration_property1_enabled: Configuration,
    ) {
        let (mut client, metering_recv) = {
            let live_cfg_mock = LiveConfigurationMock {
                configuration: example_configuration_enterprise,
            };

            let configuration_id = ConfigurationId::new(
                "test_guid".to_string(),
                "test_env_id".to_string(),
                "test_collection_id".to_string(),
            );
            let (metering, metering_recv) = start_metering_mock(configuration_id);

            (
                AppConfigurationClientHttp {
                    live_configuration: live_cfg_mock,
                    metering,
                },
                metering_recv,
            )
        };

        let property = client.get_property("p1").unwrap();

        let entity = crate::entity::tests::TrivialEntity {};
        let property_value1 = property.get_value(&entity).unwrap();

        // We simulate an update of the configuration:
        client.live_configuration = LiveConfigurationMock {
            configuration: configuration_property1_enabled,
        };
        // The property value should not have changed (as we did not retrieve it again)
        let property_value2 = property.get_value(&entity).unwrap();
        assert_eq!(property_value2, property_value1);

        // Now we retrieve the property again:
        let property = client.get_property("p1").unwrap();
        // And expect the updated value
        let property_value3 = property.get_value(&entity).unwrap();
        assert_ne!(property_value3, property_value1);

        // We evaluated the property 3 times (for two different configurations)
        {
            let metering_data = metering_recv.recv().unwrap();
            // The value for the `collection_id` and `environment_id` comes from the `ConfigurationId`
            // object that was provided to the `start_metering` function. It doesn't match
            // the `ConfigurationId` that was used to get the `Configuration` object. This
            // inconsistency is only reachable in these tests, not via the public API, so
            // there is nothing to fix right now.
            assert_eq!(metering_data.collection_id, "test_collection_id");
            assert_eq!(metering_data.environment_id, "test_env_id");

            // We expect 3 evaluations to be covered in metering.
            // Do not care about the way they are sorted.
            let total_counts: u32 = metering_data.usages.iter().map(|usage| usage.count).sum();
            assert_eq!(total_counts, 3);
        }
    }
}

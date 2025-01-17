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

use crate::client::configuration::Configuration;
pub use crate::client::feature_proxy::FeatureProxy;
use crate::client::feature_snapshot::FeatureSnapshot;
pub use crate::client::property_proxy::PropertyProxy;
use crate::client::property_snapshot::PropertySnapshot;
use crate::errors::{ConfigurationAccessError, Error, Result};
use crate::TokenProvider;
use crate::{ServerClientImpl, ServiceAddress};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;
use tungstenite::WebSocket;

use super::{AppConfigurationClient, ConfigurationId};

/// AppConfiguration client implementation that connects to a server
#[derive(Debug)]
pub struct AppConfigurationClientHttp {
    latest_config_snapshot: Arc<Mutex<Configuration>>,
    _thread_terminator: std::sync::mpsc::Sender<()>,
}

impl AppConfigurationClientHttp {
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

        // Populate initial configuration
        let latest_config_snapshot: Arc<Mutex<Configuration>> = Arc::new(Mutex::new(
            Self::get_configuration_snapshot(&server_client, &configuration_id)?,
        ));

        // start monitoring configuration
        let terminator = Self::update_cache_in_background(
            latest_config_snapshot.clone(),
            server_client,
            configuration_id,
        )?;

        let client = Self {
            latest_config_snapshot,
            _thread_terminator: terminator,
        };

        Ok(client)
    }

    fn get_configuration_snapshot(
        server_client: &ServerClientImpl,
        configuration_id: &ConfigurationId,
    ) -> Result<Configuration> {
        let configuration = server_client.get_configuration(configuration_id)?;
        Configuration::new(&configuration_id.environment_id, configuration)
    }

    fn wait_for_configuration_update(
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
        server_client_impl: &ServerClientImpl,
        configuration_id: &ConfigurationId,
    ) -> Result<Configuration> {
        loop {
            // read() blocks until something happens.
            match socket.read()? {
                Message::Text(text) => match text.as_str() {
                    "test message" => {} // periodically sent by the server
                    _ => {
                        return Self::get_configuration_snapshot(
                            server_client_impl,
                            configuration_id,
                        );
                    }
                },
                Message::Close(_) => {
                    return Err(Error::Other("Connection closed by the server".into()));
                }
                _ => {}
            }
        }
    }

    fn update_configuration_on_change(
        mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
        latest_config_snapshot: Arc<Mutex<Configuration>>,
        server_client_impl: ServerClientImpl,
        configuration_id: ConfigurationId,
    ) -> std::sync::mpsc::Sender<()> {
        let (sender, receiver) = std::sync::mpsc::channel();

        thread::spawn(move || {
            loop {
                // If the sender has gone (AppConfiguration instance is dropped), then finish this thread
                if let Err(e) = receiver.try_recv() {
                    if e == std::sync::mpsc::TryRecvError::Disconnected {
                        break;
                    }
                }

                let config_snapshot = Self::wait_for_configuration_update(
                    &mut socket,
                    &server_client_impl,
                    &configuration_id,
                );

                match config_snapshot {
                    Ok(config_snapshot) => *latest_config_snapshot.lock()? = config_snapshot,
                    Err(e) => {
                        println!("Waiting for configuration update failed. Stopping to monitor for changes.: {e}");
                        break;
                    }
                }
            }
            Ok::<(), Error>(())
        });

        sender
    }

    fn update_cache_in_background(
        latest_config_snapshot: Arc<Mutex<Configuration>>,
        server_client_impl: ServerClientImpl,
        configuration_id: ConfigurationId,
    ) -> Result<std::sync::mpsc::Sender<()>> {
        let (socket, _response) =
            server_client_impl.get_configuration_monitoring_websocket(&configuration_id)?;

        let sender = Self::update_configuration_on_change(
            socket,
            latest_config_snapshot,
            server_client_impl,
            configuration_id,
        );

        Ok(sender)
    }
}

impl AppConfigurationClient for AppConfigurationClientHttp {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .latest_config_snapshot
            .lock()?
            .features
            .keys()
            .cloned()
            .collect())
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        let config_snapshot = self.latest_config_snapshot.lock()?;

        // Get the feature from the snapshot
        let feature = config_snapshot.get_feature(feature_id)?;

        // Get the segment rules that apply to this feature
        let segments = config_snapshot.get_segments_for_segment_rules(&feature.segment_rules);

        // Integrity DB check: all segment_ids should be available in the snapshot
        if feature.segment_rules.len() != segments.len() {
            return Err(ConfigurationAccessError::MissingSegments {
                resource_id: feature_id.to_string(),
            }
            .into());
        }

        Ok(FeatureSnapshot::new(feature.clone(), segments))
    }

    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>> {
        // FIXME: there is and was no validation happening if the feature exists.
        // Comments and error messages in FeatureProxy suggest that this should happen here.
        // same applies for properties.
        Ok(FeatureProxy::new(self, feature_id.to_string()))
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .latest_config_snapshot
            .lock()
            .map_err(|_| ConfigurationAccessError::LockAcquisitionError)?
            .properties
            .keys()
            .cloned()
            .collect())
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        let config_snapshot = self.latest_config_snapshot.lock()?;

        // Get the property from the snapshot
        let property = config_snapshot.get_property(property_id)?;

        // Get the segment rules that apply to this property
        let segments = config_snapshot.get_segments_for_segment_rules(&property.segment_rules);

        // Integrity DB check: all segment_ids should be available in the snapshot
        if property.segment_rules.len() != segments.len() {
            return Err(ConfigurationAccessError::MissingSegments {
                resource_id: property_id.to_string(),
            }
            .into());
        }

        Ok(PropertySnapshot::new(property.clone(), segments))
    }

    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy> {
        Ok(PropertyProxy::new(self, property_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::tests::{
        configuration_feature1_enabled, configuration_property1_enabled,
        example_configuration_enterprise,
    };
    use crate::{models::ConfigurationJson, Feature, Property};
    use rstest::rstest;

    #[rstest]
    fn test_get_feature_persistence(
        example_configuration_enterprise: ConfigurationJson,
        configuration_feature1_enabled: ConfigurationJson,
    ) {
        let client = {
            let configuration_snapshot =
                Configuration::new("dev", example_configuration_enterprise).unwrap();

            let (sender, _) = std::sync::mpsc::channel();

            AppConfigurationClientHttp {
                latest_config_snapshot: Arc::new(Mutex::new(configuration_snapshot)),
                _thread_terminator: sender,
            }
        };

        let feature = client.get_feature("f1").unwrap();

        let entity = crate::entity::tests::TrivialEntity {};
        let feature_value1 = feature.get_value(&entity).unwrap();

        // We simulate an update of the configuration:
        let configuration_snapshot =
            Configuration::new("environment_id", configuration_feature1_enabled).unwrap();
        *client.latest_config_snapshot.lock().unwrap() = configuration_snapshot;
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
        let client = {
            let configuration_snapshot =
                Configuration::new("dev", example_configuration_enterprise).unwrap();

            let (sender, _) = std::sync::mpsc::channel();

            AppConfigurationClientHttp {
                latest_config_snapshot: Arc::new(Mutex::new(configuration_snapshot)),
                _thread_terminator: sender,
            }
        };

        let property = client.get_property("p1").unwrap();

        let entity = crate::entity::tests::TrivialEntity {};
        let property_value1 = property.get_value(&entity).unwrap();

        // We simulate an update of the configuration:
        let configuration_snapshot =
            Configuration::new("environment_id", configuration_property1_enabled).unwrap();
        *client.latest_config_snapshot.lock().unwrap() = configuration_snapshot;
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

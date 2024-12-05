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

use crate::client::cache::ConfigurationSnapshot;
use crate::client::feature::Feature;
pub use crate::client::feature_proxy::FeatureProxy;
use crate::client::http;
use crate::client::property::Property;
pub use crate::client::property_proxy::PropertyProxy;
use crate::errors::{ConfigurationAccessError, Result};
use crate::models::Segment;
use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;
use tungstenite::WebSocket;

/// App Configuration client for browsing, and evaluating features and
/// properties.
#[derive(Debug)]
pub struct AppConfigurationClient {
    pub(crate) latest_config_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
    pub(crate) _thread_terminator: std::sync::mpsc::Sender<()>,
}

impl AppConfigurationClient {
    /// Creates a client to retrieve configurations for a specific collection.
    /// To uniquely address a collection the following is required:
    /// - `region`
    /// - `guid`: Identifies an instance
    /// - `environment_id`
    /// - `collection_id`
    /// In addition `api_key` is required for authentication
    pub fn new(
        apikey: &str,
        region: &str,
        guid: &str,
        environment_id: &str,
        collection_id: &str,
    ) -> Result<Self> {
        let access_token = http::get_access_token(&apikey)?;

        // Populate initial configuration
        let latest_config_snapshot: Arc<Mutex<ConfigurationSnapshot>> =
            Arc::new(Mutex::new(Self::get_configuration_snapshot(
                &access_token,
                region,
                guid,
                environment_id,
                collection_id,
            )?));

        // start monitoring configuration
        let terminator = Self::update_cache_in_background(
            latest_config_snapshot.clone(),
            apikey,
            region,
            guid,
            environment_id,
            collection_id,
        )?;

        let client = AppConfigurationClient {
            latest_config_snapshot,
            _thread_terminator: terminator,
        };

        Ok(client)
    }

    fn get_configuration_snapshot(
        access_token: &String,
        region: &str,
        guid: &str,
        environment_id: &str,
        collection_id: &str,
    ) -> Result<ConfigurationSnapshot> {
        let configuration = http::get_configuration(
            // TODO: access_token might expire. This will cause issues with long-running apps
            &access_token,
            &region,
            &guid,
            &collection_id,
            &environment_id,
        )?;
        ConfigurationSnapshot::new(environment_id, configuration)
    }

    fn update_configuration_on_change(
        mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
        latest_config_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
        access_token: String,
        region: String,
        guid: String,
        collection_id: String,
        environment_id: String,
    ) -> std::sync::mpsc::Sender<()> {
        let (sender, receiver) = std::sync::mpsc::channel();

        thread::spawn(move || loop {
            // If the sender has gone (AppConfiguration instance is dropped), then finish this thread
            if let Err(e) = receiver.try_recv() {
                if e == std::sync::mpsc::TryRecvError::Disconnected {
                    break;
                }
            }

            // Wait for new data
            match socket.read() {
                Ok(Message::Text(text)) => match text.as_str() {
                    "test message" => {
                        println!("\t*** Test message received.");
                    }
                    _ => {
                        let config_result = Self::get_configuration_snapshot(
                            &access_token,
                            &region,
                            &guid,
                            &environment_id,
                            &collection_id,
                        );
                        let mut config_snapshot = latest_config_snapshot.lock().unwrap();
                        match config_result {
                            Ok(config) => *config_snapshot = config,
                            Err(e) => println!("Error getting config snapshot: {}", e),
                        }
                    }
                },
                Ok(Message::Close(_)) => {
                    println!("Connection closed by the server.");
                    break;
                }
                Ok(Message::Binary(data)) => {
                    println!("\t*** Received a message that has binary data {:?}", data);
                }
                Ok(Message::Ping(data)) => {
                    println!("\t*** Received a ping message {:?}", data);
                }
                Ok(Message::Pong(data)) => {
                    println!("\t*** Received a pong message {:?}", data);
                }
                Ok(Message::Frame(frame)) => {
                    println!("\t*** Received a frame message {:?}", frame);
                }
                Err(e) => {
                    // TODO: how to handle temporary connectivity issues / errors?
                    // In current implementation we would terminate this thread.
                    // Effectively freezing the configuration.
                    println!("Error: {}", e);
                    break;
                }
            }

            thread::sleep(Duration::from_millis(100));
        });

        sender
    }

    pub fn get_feature_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .latest_config_snapshot
            .lock()?
            .features
            .keys()
            .cloned()
            .collect())
    }

    pub fn get_feature(&self, feature_id: &str) -> Result<Feature> {
        let config_snapshot = self.latest_config_snapshot.lock()?;

        // Get the feature from the snapshot
        let feature = config_snapshot.get_feature(feature_id)?;

        // Get the segment rules that apply to this feature
        let segments = {
            let all_segment_ids = feature
                .segment_rules
                .iter()
                .flat_map(|targeting_rule| {
                    targeting_rule
                        .rules
                        .iter()
                        .flat_map(|segment| &segment.segments)
                })
                .cloned()
                .collect::<HashSet<String>>();
            let segments: HashMap<String, Segment> = config_snapshot
                .segments
                .iter()
                .filter(|&(key, _)| all_segment_ids.contains(key))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            // Integrity DB check: all segment_ids should be available in the snapshot
            if all_segment_ids.len() != segments.len() {
                return Err(ConfigurationAccessError::MissingSegments {
                    resource_id: feature_id.to_string(),
                }
                .into());
            }

            segments
        };

        Ok(Feature::new(feature.clone(), segments))
    }

    /// Searches for the feature `feature_id` inside the current configured
    /// collection, and environment.
    ///
    /// Return `Ok(feature)` if the feature exists or `Err` if it does not.
    pub fn get_feature_proxy(&self, feature_id: &str) -> Result<FeatureProxy> {
        // FIXME: there is and was no validation happening if the feature exists.
        // Comments and error messages in FeatureProxy suggest that this should happen here.
        // same applies for properties.
        Ok(FeatureProxy::new(
            self.latest_config_snapshot.clone(),
            feature_id.to_string(),
        ))
    }

    pub fn get_property_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .latest_config_snapshot
            .lock()
            .map_err(|_| ConfigurationAccessError::LockAcquisitionError)?
            .properties
            .keys()
            .cloned()
            .collect())
    }

    pub fn get_property(&self, property_id: &str) -> Result<Property> {
        let config_snapshot = self.latest_config_snapshot.lock()?;

        // Get the property from the snapshot
        let property = config_snapshot.get_property(property_id)?;

        // Get the segment rules that apply to this property
        let segments = {
            let all_segment_ids = property
                .segment_rules
                .iter()
                .flat_map(|targeting_rule| {
                    targeting_rule
                        .rules
                        .iter()
                        .flat_map(|segment| &segment.segments)
                })
                .cloned()
                .collect::<HashSet<String>>();
            let segments: HashMap<String, Segment> = config_snapshot
                .segments
                .iter()
                .filter(|&(key, _)| all_segment_ids.contains(key))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            // Integrity DB check: all segment_ids should be available in the snapshot
            if all_segment_ids.len() != segments.len() {
                // FIXME: Return some kind of DBIntegrity error
                return Err(ConfigurationAccessError::MissingSegments {
                    resource_id: property_id.to_string(),
                }
                .into());
            }

            segments
        };

        Ok(Property::new(property.clone(), segments))
    }

    /// Searches for the property `property_id` inside the current configured
    /// collection, and environment.
    ///
    /// Return `Ok(property)` if the feature exists or `Err` if it does not.
    pub fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy> {
        Ok(PropertyProxy::new(
            self.latest_config_snapshot.clone(),
            property_id.to_string(),
        ))
    }

    fn update_cache_in_background(
        latest_config_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
        apikey: &str,
        region: &str,
        guid: &str,
        environment_id: &str,
        collection_id: &str,
    ) -> Result<std::sync::mpsc::Sender<()>> {
        let access_token = http::get_access_token(&apikey)?;
        let (socket, _response) = http::get_configuration_monitoring_websocket(
            &access_token,
            &region,
            &guid,
            &collection_id,
            &environment_id,
        )?;

        let sender = Self::update_configuration_on_change(
            socket,
            latest_config_snapshot,
            access_token,
            region.to_string(),
            guid.to_string(),
            collection_id.to_string(),
            environment_id.to_string(),
        );

        Ok(sender)
    }
}

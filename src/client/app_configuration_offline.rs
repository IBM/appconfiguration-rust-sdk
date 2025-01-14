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
pub use crate::client::feature_proxy::FeatureProxy;
use crate::client::feature_snapshot::FeatureSnapshot;
pub use crate::client::property_proxy::PropertyProxy;
use crate::client::property_snapshot::PropertySnapshot;
use crate::errors::{ConfigurationAccessError, DeserializationError, Error, Result};
use crate::models::{Configuration, Segment};
use std::collections::{HashMap, HashSet};

use super::AppConfigurationClient;

/// AppConfiguration client using a local file with a snapshot
#[derive(Debug)]
pub struct AppConfigurationOffline {
    pub(crate) config_snapshot: ConfigurationSnapshot,
}

impl AppConfigurationOffline {
    /// Creates a new [`AppConfigurationClient`] taking the configuration from a local file.
    ///
    /// # Arguments
    ///
    /// * `filepath` - The file with the configuration.
    /// * `region` - Region name where the App Configuration service instance is created
    /// * `guid` - Instance ID of the App Configuration service. Obtain it from the service credentials section of the App Configuration dashboard
    /// * `environment_id` - ID of the environment created in App Configuration service instance under the Environments section.
    /// * `collection_id` - ID of the collection created in App Configuration service instance under the Collections section
    pub fn new(filepath: &std::path::Path, environment_id: &str) -> Result<Self> {
        let file = std::fs::File::open(filepath).map_err(|_| {
            Error::Other(format!(
                "File '{}' doesn't exist or cannot be read",
                filepath.display()
            ))
        })?;
        let reader = std::io::BufReader::new(file);

        let configuration: Configuration = serde_json::from_reader(reader).map_err(|e| {
            Error::DeserializationError(DeserializationError {
                string: format!(
                    "Error deserializing Configuration from file '{}'",
                    filepath.display()
                ),
                source: e.into(),
            })
        })?;
        let config_snapshot = ConfigurationSnapshot::new(environment_id, configuration)?;

        Ok(Self { config_snapshot })
    }
}

impl AppConfigurationClient for AppConfigurationOffline {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        Ok(self.config_snapshot.features.keys().cloned().collect())
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        // Get the feature from the snapshot
        let feature = self.config_snapshot.get_feature(feature_id)?;

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
            let segments: HashMap<String, Segment> = self
                .config_snapshot
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

        Ok(FeatureSnapshot::new(feature.clone(), segments))
    }

    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>> {
        // FIXME: there is and was no validation happening if the feature exists.
        // Comments and error messages in FeatureProxy suggest that this should happen here.
        // same applies for properties.
        Ok(FeatureProxy::new(self, feature_id.to_string()))
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        Ok(self.config_snapshot.properties.keys().cloned().collect())
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        // Get the property from the snapshot
        let property = self.config_snapshot.get_property(property_id)?;

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
            let segments: HashMap<String, Segment> = self
                .config_snapshot
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

        Ok(PropertySnapshot::new(property.clone(), segments))
    }

    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy> {
        Ok(PropertyProxy::new(self, property_id.to_string()))
    }
}

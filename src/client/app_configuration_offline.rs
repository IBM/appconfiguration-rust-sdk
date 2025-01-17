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
use crate::errors::{DeserializationError, Error, Result};
use crate::models::ConfigurationJson;

use super::AppConfigurationClient;

/// AppConfiguration client using a local file with a configuration snapshot
#[derive(Debug)]
pub struct AppConfigurationOffline {
    pub(crate) config_snapshot: Configuration,
}

impl AppConfigurationOffline {
    /// Creates a new [`AppConfigurationClient`] taking the configuration from a local file.
    ///
    /// # Arguments
    ///
    /// * `filepath` - The file with the configuration.
    /// * `environment_id` - ID of the environment to use from the configuration file.
    pub fn new(filepath: &std::path::Path, environment_id: &str) -> Result<Self> {
        let file = std::fs::File::open(filepath).map_err(|_| {
            Error::Other(format!(
                "File '{}' doesn't exist or cannot be read",
                filepath.display()
            ))
        })?;
        let reader = std::io::BufReader::new(file);

        let configuration: ConfigurationJson = serde_json::from_reader(reader).map_err(|e| {
            Error::DeserializationError(DeserializationError {
                string: format!(
                    "Error deserializing Configuration from file '{}'",
                    filepath.display()
                ),
                source: e.into(),
            })
        })?;
        let config_snapshot = Configuration::new(environment_id, configuration)?;

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
        let segments = self
            .config_snapshot
            .get_segments_for_segment_rules(&feature.segment_rules);

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

        // Get the segment rules that apply to this feature
        let segments = self
            .config_snapshot
            .get_segments_for_segment_rules(&property.segment_rules);

        Ok(PropertySnapshot::new(property.clone(), segments))
    }

    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy> {
        Ok(PropertyProxy::new(self, property_id.to_string()))
    }
}

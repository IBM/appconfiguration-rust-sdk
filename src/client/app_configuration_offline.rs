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

use crate::ConfigurationProvider;
use crate::errors::Result;
use crate::models::{Configuration, FeatureSnapshot, PropertySnapshot, SecretPropertySnapshot};

/// AppConfiguration client using a local file with a configuration snapshot
#[derive(Debug)]
pub struct AppConfigurationOffline {
    pub(crate) config_snapshot: Configuration,
}

impl AppConfigurationOffline {
    /// Creates a new [`crate::AppConfigurationClient`] taking the configuration from a local file.
    ///
    /// # Arguments
    ///
    /// * `filepath` - The file with the configuration.
    /// * `environment_id` - ID of the environment to use from the configuration file.
    pub fn new(
        filepath: &std::path::Path,
        environment_id: &str,
        collection_id: &str,
    ) -> Result<Self> {
        let config_snapshot = Configuration::from_file(filepath, environment_id, collection_id)?;
        Ok(Self { config_snapshot })
    }
}

impl ConfigurationProvider for AppConfigurationOffline {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        self.config_snapshot.get_feature_ids()
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        self.config_snapshot.get_feature(feature_id)
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        self.config_snapshot.get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        self.config_snapshot.get_property(property_id)
    }

    fn is_online(&self) -> Result<bool> {
        Ok(false)
    }

    fn wait_until_online(&self) -> bool {
        // AppConfigurationOffline never connects to a remote server;
        // waiting for "online" is meaningless — return false immediately.
        false
    }
    fn get_secret_property(&self, property_id: &str) -> Result<SecretPropertySnapshot> {
        self.config_snapshot.get_secret_property(property_id)
    }

    fn clean_up(&mut self) -> Result<()> {
        Ok(())
    }

    fn clean_up_with_cache_clear(&mut self) -> Result<()> {
        Ok(())
    }
}

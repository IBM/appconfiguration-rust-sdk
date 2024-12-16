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

use crate::AppConfigurationClient;
use crate::Result;

pub trait AppConfigClientBuilder {
    /// Creates and returns the [`AppConfigurationClient`]
    fn build(self) -> Result<AppConfigurationClient>;
}

/// An [`AppConfigurationClient`] builder to create a client connecting to IBM Cloud
pub struct AppConfigIBMCloudBuilder<'a> {
    region: &'a str,
    apikey: &'a str,
    guid: &'a str,
    environment_id: &'a str,
    collection_id: &'a str,
}

impl<'a> AppConfigIBMCloudBuilder<'a> {
    /// Creates a builder with the data required to instantiate a new [`AppConfigClientBuilder`]
    /// connecting to IBM Cloud
    ///
    /// # Arguments
    ///
    /// * `region` - Region name where the App Configuration service instance is created
    /// * `apikey` - The encrypted API key.
    /// * `guid` - Instance ID of the App Configuration service. Obtain it from the service credentials section of the App Configuration dashboard
    /// * `collection_id` - ID of the collection created in App Configuration service instance under the Collections section
    /// * `environment_id` - ID of the environment created in App Configuration service instance under the Environments section.
    pub fn new(
        region: &'a str,
        apikey: &'a str,
        guid: &'a str,
        environment_id: &'a str,
        collection_id: &'a str,
    ) -> Self {
        Self {
            region,
            apikey,
            guid,
            environment_id,
            collection_id,
        }
    }
}

impl<'a> AppConfigClientBuilder for AppConfigIBMCloudBuilder<'a> {
    fn build(self) -> Result<AppConfigurationClient> {
        AppConfigurationClient::new(
            self.apikey,
            self.region,
            self.guid,
            self.environment_id,
            self.collection_id,
        )
    }
}

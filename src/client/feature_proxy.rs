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

use super::AppConfigurationClient;
use crate::entity::Entity;
use crate::models::FeatureSnapshot;
use crate::value::Value;
use crate::{Feature, FeatureEvaluationResult};
/// Provides live-updated data for a given [`Feature`].
pub struct FeatureProxy<'a> {
    client: &'a dyn AppConfigurationClient,
    feature_id: String,
}

impl<'a> FeatureProxy<'a> {
    pub(crate) fn new(client: &'a dyn AppConfigurationClient, feature_id: String) -> Self {
        Self { client, feature_id }
    }

    /// Take a snapshot of this proxied property
    pub fn snapshot(&self) -> crate::errors::Result<FeatureSnapshot> {
        self.client.get_feature(&self.feature_id)
    }
}

impl Feature for FeatureProxy<'_> {
    fn get_feature_name(&self) -> crate::errors::Result<String> {
        self.client
            .get_feature(&self.feature_id)?
            .get_feature_name()
    }

    fn is_enabled(&self) -> crate::errors::Result<bool> {
        self.client.get_feature(&self.feature_id)?.is_enabled()
    }

    fn get_feature_id(&self) -> crate::errors::Result<String> {
        self.client.get_feature(&self.feature_id)?.get_feature_id()
    }

    fn get_feature_data_type(&self) -> crate::errors::Result<String> {
        self.client
            .get_feature(&self.feature_id)?
            .get_feature_data_type()
    }

    fn get_feature_data_format(&self) -> crate::errors::Result<Option<String>> {
        self.client
            .get_feature(&self.feature_id)?
            .get_feature_data_format()
    }

    fn get_current_value(
        &self,
        entity: &impl Entity,
    ) -> crate::errors::Result<FeatureEvaluationResult> {
        self.client
            .get_feature(&self.feature_id)?
            .get_current_value(entity)
    }

    fn get_value_into<T: TryFrom<Value, Error = crate::Error>>(
        &self,
        entity: &impl Entity,
    ) -> crate::errors::Result<T> {
        self.client
            .get_feature(&self.feature_id)?
            .get_value_into(entity)
    }
}

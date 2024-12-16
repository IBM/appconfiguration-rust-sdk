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

use crate::Result;

use crate::client::feature_proxy::FeatureProxy;
use crate::client::feature_snapshot::FeatureSnapshot;
use crate::client::property_proxy::PropertyProxy;
use crate::client::property_snapshot::PropertySnapshot;

/// AppConfiguration client for browsing, and evaluating features and properties.
pub trait AppConfigurationClient {
    fn get_feature_ids(&self) -> Result<Vec<String>>;

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot>;

    /// Searches for the feature `feature_id` inside the current configured
    /// collection, and environment.
    ///
    /// Return `Ok(feature)` if the feature exists or `Err` if it does not.
    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>>;

    fn get_property_ids(&self) -> Result<Vec<String>>;

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot>;

    /// Searches for the property `property_id` inside the current configured
    /// collection, and environment.
    ///
    /// Return `Ok(property)` if the feature exists or `Err` if it does not.
    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy>;
}

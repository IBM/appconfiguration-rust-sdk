// (C) Copyright IBM Corp. 2025.
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

use crate::client::feature_snapshot::FeatureSnapshot;

use crate::client::property_snapshot::PropertySnapshot;

pub trait ConfigurationProvider {
    /// Returns the list of features.
    ///
    /// The list contains the `id`s that can be used in other methods to return
    /// concrete features, like [`get_feature`](ConfigurationProvider::get_feature).
    fn get_feature_ids(&self) -> Result<Vec<String>>;

    /// Returns a snapshot for a [`Feature`](crate::Feature).
    ///
    /// The instance contains a snapshot with all the values and rules, so it
    /// will always evaluate the same entities to the same values, no updates
    /// will be received from the server.
    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot>;

    /// Returns the list of properties.
    ///
    /// The list contains the `id`s that can be used in other methods to return
    /// concrete properties, like [`get_property`](ConfigurationProvider::get_property).
    fn get_property_ids(&self) -> Result<Vec<String>>;

    /// Returns a snapshot for a [`Property`](crate::Property).
    ///
    /// The instance contains a snapshot with all the values and rules, so it
    /// will always evaluate the same entities to the same values, no updates
    /// will be received from the server
    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot>;
}

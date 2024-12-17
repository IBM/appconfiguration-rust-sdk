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

use std::io::Cursor;

use murmur3::murmur3_32;

use crate::entity::Entity;
use crate::{Feature, Value};

use super::feature_snapshot::FeatureSnapshot;
use super::AppConfigurationClient;

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

impl<'a> Feature for FeatureProxy<'a> {
    fn get_name(&self) -> crate::errors::Result<String> {
        self.client.get_feature(&self.feature_id)?.get_name()
    }

    fn is_enabled(&self) -> crate::errors::Result<bool> {
        self.client.get_feature(&self.feature_id)?.is_enabled()
    }

    fn get_value(&self, entity: &impl Entity) -> crate::errors::Result<Value> {
        self.client.get_feature(&self.feature_id)?.get_value(entity)
    }

    fn get_value_into<T: TryFrom<Value, Error = crate::Error>>(&self, entity: &impl Entity) -> crate::errors::Result<T> {
        self.client.get_feature(&self.feature_id)?.get_value_into(entity)
    }
}

pub(crate) fn random_value(v: &str) -> u32 {
    let max_hash = u32::MAX;
    (f64::from(hash(v)) / f64::from(max_hash) * 100.0) as u32
}

fn hash(v: &str) -> u32 {
    murmur3_32(&mut Cursor::new(v), 0).expect("Cannot hash the value.")
}

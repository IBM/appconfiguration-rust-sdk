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

use std::collections::HashMap;

mod test_get_feature;
mod test_get_feature_ids;
mod test_get_property;
mod test_get_property_ids;
mod test_using_example_data;

use crate::client::cache::ConfigurationSnapshot;
use crate::client::AppConfigurationClient;
use crate::models::tests::example_configuration_enterprise;
use crate::models::Configuration;
use crate::Entity;
use rstest::fixture;
use crate::Value;
use std::sync::{Arc, Mutex};

pub struct TrivialEntity;

impl Entity for TrivialEntity {
    fn get_id(&self) -> String {
        "TrivialId".into()
    }

    fn get_attributes(&self) -> HashMap<String, Value> {
        HashMap::new()
    }
}

pub struct GenericEntity {
    pub id: String,
    pub attributes: HashMap<String, Value>,
}

impl Entity for GenericEntity {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_attributes(&self) -> HashMap<String, Value> {
        self.attributes.clone()
    }
}

#[fixture]
fn client_enterprise(example_configuration_enterprise: Configuration) -> AppConfigurationClient {
    let configuration_snapshot =
        ConfigurationSnapshot::new("dev", example_configuration_enterprise).unwrap();

    // Create the client
    let (sender, _) = std::sync::mpsc::channel();

    AppConfigurationClient {
        latest_config_snapshot: Arc::new(Mutex::new(configuration_snapshot)),
        _thread_terminator: sender,
    }
}

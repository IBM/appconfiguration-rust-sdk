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

use crate::network::models::ConfigurationJson;

use crate::client::configuration::Configuration;
use crate::client::{AppConfigurationClient, AppConfigurationOffline};
use crate::{ConfigurationProvider, Value};
use rstest::*;

use super::client_enterprise;
use crate::network::models::tests::configuration_unordered_segment_rules;
use crate::property::Property;

#[rstest]
fn test_get_property_doesnt_exist(client_enterprise: Box<dyn AppConfigurationClient>) {
    let property = client_enterprise.get_property("non-existing");
    assert!(property.is_err());
    assert_eq!(
        property.unwrap_err().to_string(),
        "Property `non-existing` not found."
    );
}

#[rstest]
fn test_get_property_ordered(configuration_unordered_segment_rules: ConfigurationJson) {
    let config_snapshot =
        Configuration::new("environment_id", configuration_unordered_segment_rules).unwrap();

    let client = AppConfigurationOffline { config_snapshot };

    let entity = crate::tests::GenericEntity {
        id: "a2".into(),
        attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
    };
    let value = client
        .get_property("f1")
        .unwrap()
        .get_value(&entity)
        .unwrap();
    assert!(matches!(value, Value::Int64(ref v) if v == &(-49)));
}

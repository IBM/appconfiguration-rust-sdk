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
use std::sync::{Arc, Mutex};

use crate::models::Configuration;

use crate::client::cache::ConfigurationSnapshot;
use crate::client::{AppConfigurationClient, AppConfigurationClientHttp};
use crate::Value;
use rstest::*;

use super::client_enterprise;
use crate::models::tests::{
    configuration_property1_enabled, configuration_unordered_segment_rules,
};
use crate::property::Property;

#[rstest]
fn test_get_property_persistence(
    client_enterprise: AppConfigurationClientHttp,
    configuration_property1_enabled: Configuration,
) {
    let property = client_enterprise.get_property("p1").unwrap();

    let entity = super::TrivialEntity {};
    let property_value1 = property.get_value(&entity).unwrap();

    // We simulate an update of the configuration:
    let configuration_snapshot =
        ConfigurationSnapshot::new("environment_id", configuration_property1_enabled).unwrap();
    *client_enterprise.latest_config_snapshot.lock().unwrap() = configuration_snapshot;
    // The property value should not have changed (as we did not retrieve it again)
    let property_value2 = property.get_value(&entity).unwrap();
    assert_eq!(property_value2, property_value1);

    // Now we retrieve the property again:
    let property = client_enterprise.get_property("p1").unwrap();
    // And expect the updated value
    let property_value3 = property.get_value(&entity).unwrap();
    assert_ne!(property_value3, property_value1);
}

#[rstest]
fn test_get_property_doesnt_exist(client_enterprise: AppConfigurationClientHttp) {
    let property = client_enterprise.get_property("non-existing");
    assert!(property.is_err());
    assert_eq!(
        property.unwrap_err().to_string(),
        "Property `non-existing` not found."
    );
}

#[rstest]
fn test_get_property_ordered(configuration_unordered_segment_rules: Configuration) {
    let configuration_snapshot =
        ConfigurationSnapshot::new("environment_id", configuration_unordered_segment_rules)
            .unwrap();

    // Create the client
    let (sender, _) = std::sync::mpsc::channel();

    let client = AppConfigurationClientHttp {
        latest_config_snapshot: Arc::new(Mutex::new(configuration_snapshot)),
        _thread_terminator: sender,
    };

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

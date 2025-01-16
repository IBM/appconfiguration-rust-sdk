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
use crate::client::{AppConfigurationClient, AppConfigurationClientIBMCloud};
use crate::Value;
use rstest::*;

use super::client_enterprise;
use crate::feature::Feature;
use crate::models::tests::{configuration_feature1_enabled, configuration_unordered_segment_rules};

#[rstest]
fn test_get_feature_persistence(
    client_enterprise: AppConfigurationClientIBMCloud,
    configuration_feature1_enabled: Configuration,
) {
    let feature = client_enterprise.get_feature("f1").unwrap();

    let entity = super::TrivialEntity {};
    let feature_value1 = feature.get_value(&entity).unwrap();

    // We simulate an update of the configuration:
    let configuration_snapshot =
        ConfigurationSnapshot::new("environment_id", configuration_feature1_enabled).unwrap();
    *client_enterprise.latest_config_snapshot.lock().unwrap() = configuration_snapshot;
    // The feature value should not have changed (as we did not retrieve it again)
    let feature_value2 = feature.get_value(&entity).unwrap();
    assert_eq!(feature_value2, feature_value1);

    // Now we retrieve the feature again:
    let feature = client_enterprise.get_feature("f1").unwrap();
    // And expect the updated value
    let feature_value3 = feature.get_value(&entity).unwrap();
    assert_ne!(feature_value3, feature_value1);
}

#[rstest]
fn test_get_feature_doesnt_exist(client_enterprise: AppConfigurationClientIBMCloud) {
    let feature = client_enterprise.get_feature("non-existing");
    assert!(feature.is_err());
    assert_eq!(
        feature.unwrap_err().to_string(),
        "Feature `non-existing` not found."
    );
}

#[rstest]
fn test_get_feature_ordered(configuration_unordered_segment_rules: Configuration) {
    let configuration_snapshot =
        ConfigurationSnapshot::new("environment_id", configuration_unordered_segment_rules)
            .unwrap();

    // Create the client
    let (sender, _) = std::sync::mpsc::channel();

    let client = AppConfigurationClientIBMCloud {
        latest_config_snapshot: Arc::new(Mutex::new(configuration_snapshot)),
        _thread_terminator: sender,
    };

    let entity = crate::tests::GenericEntity {
        id: "a2".into(),
        attributes: HashMap::from([("name".into(), Value::from("heinz".to_string()))]),
    };
    let value = client
        .get_feature("f1")
        .unwrap()
        .get_value(&entity)
        .unwrap();
    assert!(matches!(value, Value::Int64(ref v) if v == &(-49)));
}

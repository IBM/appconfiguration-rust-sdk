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

use dotenvy::dotenv;
use rstest::*;

use appconfiguration_rust_sdk::{
    AppConfigurationClient, AppConfigurationClientIBMCloud, Entity, Feature, Property, Value,
};
use std::collections::HashMap;
use std::env;

struct TrivialEntity;

impl Entity for TrivialEntity {
    fn get_id(&self) -> String {
        "TrivialId".into()
    }

    fn get_attributes(&self) -> HashMap<String, Value> {
        HashMap::new()
    }
}

#[fixture]
fn setup_client() -> AppConfigurationClientIBMCloud {
    dotenv().expect(
        ".env file not found. Create one with the required variables in order to run the tests.",
    );
    let region = env::var("REGION").expect("REGION should be set.");
    let guid = env::var("GUID").expect("GUID should be set.");
    let apikey = env::var("APIKEY").expect("APIKEY should be set.");

    //TODO: Our current pricing plan doesn't allow more than 1 collection, so we are using
    // car-rentals so far.
    AppConfigurationClientIBMCloud::new(&apikey, &region, &guid, "testing", "car-rentals").unwrap()
}

#[rstest]
fn test_get_list_of_features(setup_client: AppConfigurationClientIBMCloud) {
    let features = setup_client.get_feature_ids().unwrap();

    assert_eq!(features.len(), 4);
}

#[rstest]
fn test_get_a_specific_feature(setup_client: AppConfigurationClientIBMCloud) {
    let specific_feature = setup_client
        .get_feature_proxy("test-feature-flag-1")
        .unwrap();

    let name = specific_feature.get_name().unwrap();
    let value = specific_feature.get_value(&TrivialEntity).unwrap();
    let is_enabled = specific_feature.is_enabled().unwrap();

    assert_eq!(name, "test feature flag 1".to_string());
    assert!(matches!(value, Value::Boolean(ref v) if v == &false));
    assert!(!is_enabled);
}

#[rstest]
fn test_get_list_of_properties(setup_client: AppConfigurationClientIBMCloud) {
    let properties = setup_client.get_property_ids().unwrap();

    assert_eq!(properties.len(), 2);
}

#[rstest]
fn test_get_a_specific_property(setup_client: AppConfigurationClientIBMCloud) {
    let property = setup_client.get_property_proxy("test-property-1").unwrap();

    let name = property.get_name().unwrap();
    let value = property.get_value(&TrivialEntity).unwrap();

    assert_eq!(name, "Test Property 1");
    assert!(matches!(value, Value::Boolean(_)));
}

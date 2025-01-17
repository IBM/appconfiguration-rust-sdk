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

use crate::client::{AppConfigurationClient, AppConfigurationClientHttp};
use crate::tests::TrivialEntity;
use rstest::*;

use super::client_enterprise;
use crate::{Feature, Property, Value};

#[rstest]
fn test_get_a_specific_feature(client_enterprise: AppConfigurationClientHttp) {
    let specific_feature = client_enterprise.get_feature_proxy("f1").unwrap();

    let name = specific_feature.get_name().unwrap();
    let is_enabled = specific_feature.is_enabled().unwrap();
    let value = specific_feature.get_value(&TrivialEntity).unwrap();

    assert_eq!(name, "F1".to_string());
    assert!(is_enabled);
    assert!(matches!(value, Value::Int64(ref v) if v == &5));
}

#[rstest]
fn test_get_a_specific_property(client_enterprise: AppConfigurationClientHttp) {
    let property = client_enterprise.get_property_proxy("p1").unwrap();

    let name = property.get_name().unwrap();
    let value = property.get_value(&TrivialEntity).unwrap();

    assert_eq!(name, "p1");
    assert!(matches!(value, Value::Int64(ref v) if v == &5));
}

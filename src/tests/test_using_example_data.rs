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

use crate::client::AppConfigurationClient;

use rstest::*;

use super::client_enterprise;
use crate::{Feature, Property};

#[rstest]
fn test_get_a_specific_feature(client_enterprise: AppConfigurationClient) {
    use crate::models::ValueKind;
    let specific_feature = client_enterprise.get_feature_proxy("f1").unwrap();

    let name = specific_feature.get_name().unwrap();
    let data_type = specific_feature.get_data_type().unwrap();
    let is_enabled = specific_feature.is_enabled().unwrap();

    assert_eq!(name, "F1".to_string());
    assert_eq!(data_type, ValueKind::Numeric);
    assert_eq!(is_enabled, true);
    assert_eq!(specific_feature.get_enabled_value().unwrap().as_i64().unwrap(), 5);
}

#[rstest]
fn test_get_a_specific_property(client_enterprise: AppConfigurationClient) {
    use crate::models::ValueKind;
    let property = client_enterprise.get_property_proxy("p1").unwrap();

    let name = property.get_name().unwrap();
    let data_type = property.get_data_type().unwrap();

    assert_eq!(name, "p1");
    assert_eq!(data_type, ValueKind::Numeric);
    assert_eq!(property.get_value_default().unwrap().as_u64().unwrap(), 5);
}

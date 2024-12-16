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

use super::client_enterprise;
use crate::client::{AppConfigurationClient, AppConfigurationClientIBMCloud};
use rstest::*;

#[rstest]
fn test_get_property_ids(client_enterprise: AppConfigurationClientIBMCloud) {
    let mut properties = client_enterprise.get_property_ids().unwrap();
    properties.sort();
    assert_eq!(
        properties,
        vec![
            "p1".to_string(),
            "p2".to_string(),
            "p3".to_string(),
            "p4".to_string(),
        ]
    );
}

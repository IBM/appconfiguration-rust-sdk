// Copyright 2026 IBM Corp. All Rights Reserved.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

//       http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::client::app_configuration_http::AppConfigurationClientHttp;
use crate::network::{NetworkResult, ServiceAddress, TokenProvider};
use crate::{AppConfigurationClient, Result};
use crate::{ConfigurationId, OfflineMode};
#[derive(Debug, Clone)]
struct MockTokenProvider {}

impl TokenProvider for MockTokenProvider {
    fn get_access_token(&self) -> NetworkResult<String> {
        Ok("mock_token".into())
    }
}

/// Creates and returns an [`AppConfigurationClient`]-like object that connects to
/// the given server.
pub fn create_app_configuration_client_live(
    service_address: ServiceAddress,
    configuration_id: ConfigurationId,
    offline_mode: OfflineMode,
) -> Result<Box<dyn AppConfigurationClient>> {
    let token_provider = Box::new(MockTokenProvider {});
    let client = AppConfigurationClientHttp::new(
        service_address,
        token_provider,
        configuration_id,
        offline_mode,
        crate::RuntimeEventEmitter::new(),
    )?;

    Ok(Box::new(client))
}

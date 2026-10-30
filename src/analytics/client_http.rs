// (C) Copyright IBM Corp. 2025.
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

//! HTTP implementation of [`AnalyticsClient`].
//! POSTs to `/apprapp/metrics/v1/instances/{guid}/analytics`.

use std::sync::Arc;

use crate::analytics::client::AnalyticsClient;
use crate::analytics::errors::{AnalyticsError, AnalyticsResult};
use crate::analytics::record::AnalyticsPayload;
use crate::network::NetworkError;
use crate::network::http_client::SDK_USER_AGENT;
use crate::network::{ServiceAddress, ServiceAddressProtocol, TokenProvider};
use reqwest::blocking::Client;

#[derive(Debug)]
pub(crate) struct AnalyticsClientHttp {
    service_address: ServiceAddress,
    token_provider: Arc<Box<dyn TokenProvider>>,
}

impl AnalyticsClientHttp {
    pub(crate) fn new(
        service_address: ServiceAddress,
        token_provider: Arc<Box<dyn TokenProvider>>,
    ) -> Self {
        Self {
            service_address,
            token_provider,
        }
    }
}

impl AnalyticsClient for AnalyticsClientHttp {
    fn push_analytics_data(&self, guid: &str, payload: &AnalyticsPayload) -> AnalyticsResult<()> {
        // Endpoint: /apprapp/metrics/v1/instances/{guid}/analytics
        let url = format!(
            "{}/metrics/v1/instances/{}/analytics",
            self.service_address.base_url(ServiceAddressProtocol::Http),
            guid
        );

        let client = Client::new();
        let response = client
            .post(&url)
            .header("User-Agent", SDK_USER_AGENT)
            .bearer_auth(self.token_provider.get_access_token()?)
            .json(payload)
            .send()
            .map_err(NetworkError::ReqwestError)?;

        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(AnalyticsError::DataNotAccepted(status.to_string()))
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::record::AnalyticsRecord;
    use crate::network::TokenProvider;
    use httpmock::Method::POST;
    use httpmock::MockServer;
    use std::sync::Arc;

    #[derive(Default, Debug, Clone)]
    struct MockTokenProvider;

    impl TokenProvider for MockTokenProvider {
        fn get_access_token(&self) -> crate::network::NetworkResult<String> {
            Ok("mocked_token".to_string())
        }
    }

    #[test]
    fn test_well_formed_post_request() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/apprapp/metrics/v1/instances/example_guid/analytics")
                .header("content-type", "application/json")
                .header("Authorization", "Bearer mocked_token");
            then.status(200);
        });

        let client = AnalyticsClientHttp::new(
            ServiceAddress::new_without_ssl(server.host(), Some(server.port()), Some("apprapp".into())),
            Arc::new(Box::new(MockTokenProvider)),
        );

        let payload = AnalyticsRecord::new().as_payload("env", "col");
        let result = client.push_analytics_data("example_guid", &payload);

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn test_error_on_bad_status() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST);
            then.status(400);
        });

        let client = AnalyticsClientHttp::new(
            ServiceAddress::new_without_ssl(server.host(), Some(server.port()), Some("apprapp".into())),
            Arc::new(Box::new(MockTokenProvider)),
        );

        let payload = AnalyticsRecord::new().as_payload("env", "col");
        let result = client.push_analytics_data("example_guid", &payload);

        assert!(matches!(result, Err(AnalyticsError::DataNotAccepted(_))));
        mock.assert();
    }
}

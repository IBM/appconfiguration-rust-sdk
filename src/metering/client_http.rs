use crate::metering::models::MeteringDataJson;
use crate::metering::{MeteringClient, MeteringError, MeteringResult};
use crate::network::NetworkError;
use crate::network::{ServiceAddress, ServiceAddressProtocol, TokenProvider};
use reqwest::blocking::Client;
use url::Url;

/// A MeteringClient pushing metering data to a http server.
#[derive(Debug)]
pub(crate) struct MeteringClientHttp {
    service_address: ServiceAddress,
    token_provider: Box<dyn TokenProvider>,
}

impl MeteringClientHttp {
    pub(crate) fn new(
        service_address: ServiceAddress,
        token_provider: Box<dyn TokenProvider>,
    ) -> MeteringClientHttp {
        Self {
            service_address,
            token_provider,
        }
    }
}

impl MeteringClient for MeteringClientHttp {
    fn push_metering_data(&self, guid: &String, data: &MeteringDataJson) -> MeteringResult<()> {
        // TODO: implement token renewal.
        // For now get a new access token each time, avoiding the need for renewals. We don't expect high
        // frequency calls for metering, so it should be OK for now, but once we implement renewals for
        // the config endpoint, we might want to change it here too:
        let token = self.token_provider.get_access_token()?;

        let url = format!(
            "{}/apprapp/events/v1/instances/{}/usage",
            self.service_address.base_url(ServiceAddressProtocol::Http),
            guid
        );
        let url = Url::parse(&url).map_err(|_| NetworkError::UrlParseError(url))?;
        let client = Client::new();
        let r = client
            .post(url)
            .header("User-Agent", "appconfiguration-rust-sdk/0.0.1")
            .bearer_auth(&token)
            .json(data)
            .send();

        match r {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    Ok(())
                } else {
                    Err(MeteringError::DataNotAccepted(status.to_string()))
                }
            }
            Err(e) => Err(NetworkError::ReqwestError(e).into()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use httpmock::Method::POST;
    use httpmock::MockServer;
    use serde_json::json;
    #[derive(Debug, Clone)]
    struct MockTokenProvider {}

    impl TokenProvider for MockTokenProvider {
        fn get_access_token(&self) -> crate::network::NetworkResult<String> {
            Ok("mocked_token".to_string())
        }
    }

    /// Tests the good-case and asserts that the HTTP request sent to the server is well-formed:
    /// - Correct endpoint
    /// - Correct content-type
    /// - Correct authorization
    /// - Correct json serialization
    #[test]
    fn test_well_formed_post_request() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/apprapp/events/v1/instances/example_guid/usage")
                .header("content-type", "application/json")
                .header("Authorization", "Bearer mocked_token")
                .json_body(json!(
                    {
                    "collection_id": "test",
                    "environment_id": "dev",
                    "usages": []
                    }
                ));
            then.status(200);
        });

        let client = MeteringClientHttp::new(
            ServiceAddress::new_without_ssl(server.host(), Some(server.port()), None),
            Box::new(MockTokenProvider {}),
        );

        let data = MeteringDataJson {
            collection_id: "test".to_string(),
            environment_id: "dev".to_string(),
            usages: Vec::new(),
        };

        let result = client.push_metering_data(&"example_guid".to_string(), &data);

        assert!(result.is_ok());
        mock.assert();
    }

    /// In case of the server returning a bad status, `push_metering_data` should fail.
    #[test]
    fn test_error_handling() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST);
            then.status(400);
        });

        let client = MeteringClientHttp::new(
            ServiceAddress::new_without_ssl(server.host(), Some(server.port()), None),
            Box::new(MockTokenProvider {}),
        );

        let data = MeteringDataJson {
            collection_id: "test".to_string(),
            environment_id: "dev".to_string(),
            usages: Vec::new(),
        };

        let result = client.push_metering_data(&"example_guid".to_string(), &data);

        assert!(matches!(result, Err(MeteringError::DataNotAccepted(_))));
        mock.assert();
    }
}

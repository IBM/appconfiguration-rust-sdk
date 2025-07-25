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

use super::MeteringResult;
use crate::models::MeteringDataJson;
use crate::network::{ServiceAddress, TokenProvider};
use std::cell::RefCell;

pub(crate) trait MeteringClient: Send + 'static {
    fn push_metering_data(&self, _guid: &String, _data: &MeteringDataJson) -> MeteringResult<()>;
}

#[derive(Debug)]
pub(crate) struct MeteringClientImpl {
    service_address: ServiceAddress,
    token_provider: Box<dyn TokenProvider>,

    // FIXME: If we test that this object is not Send+Sync, is it safe to
    // assume that the RefCell will never be borrowed and replaced at the
    // same time?
    access_token: RefCell<String>,
}

impl MeteringClientImpl {
    pub(crate) fn new(
        service_address: ServiceAddress,
        token_provider: Box<dyn TokenProvider>,
    ) -> MeteringClientImpl {
        todo!()
    }
}

impl MeteringClient for MeteringClientImpl {
    fn push_metering_data(&self, _guid: &String, _data: &MeteringDataJson) -> MeteringResult<()> {
        todo!()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use crate::metering::MeteringResult;
    use crate::models::MeteringDataJson;
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

    #[test]
    fn test_push_metering_data() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/apprapp/events/v1/instances/example_guid/usage")
                .header("content-type", "application/json")
                .json_body(json!(
                    {
                    "collection_id": "test",
                    "environment_id": "dev",
                    "usages": []
                    }
                ));
            then.status(200);
        });

        let client = MeteringClientImpl::new(
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
}

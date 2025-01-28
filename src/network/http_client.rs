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

use super::TokenProvider;
use crate::models::ConfigurationJson;
use crate::{ConfigurationId, Error, Result};
use reqwest::blocking::Client;
use std::cell::RefCell;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;

use std::net::TcpStream;

use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Response;

use tungstenite::connect;
use url::Url;

pub enum ServiceAddressProtocol {
    Http,
    Ws,
}

impl std::fmt::Display for ServiceAddressProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceAddressProtocol::Http => write!(f, "http"),
            ServiceAddressProtocol::Ws => write!(f, "ws"),
        }
    }
}

#[derive(Debug)]
pub struct ServiceAddress {
    host: String,
    port: Option<u16>,
    endpoint: Option<String>,
    use_ssl: bool,
}

impl ServiceAddress {
    pub fn new(host: String, port: Option<u16>, endpoint: Option<String>) -> Self {
        Self {
            host,
            port,
            endpoint,
            use_ssl: true,
        }
    }

    pub fn new_without_ssl(host: String, port: Option<u16>, endpoint: Option<String>) -> Self {
        Self {
            host,
            port,
            endpoint,
            use_ssl: false,
        }
    }

    pub(crate) fn base_url(&self, protocol: ServiceAddressProtocol) -> String {
        let port = if let Some(port) = self.port {
            format!(":{port}")
        } else {
            "".to_string()
        };

        let endpoint = if let Some(endpoint) = &self.endpoint {
            format!("/{endpoint}")
        } else {
            "".to_string()
        };
        let ssl_suffix = if self.use_ssl { "s" } else { "" };
        format!("{protocol}{ssl_suffix}://{}{port}{endpoint}", self.host)
    }
}

#[derive(Debug)]
pub(crate) struct ServerClientImpl {
    service_address: ServiceAddress,
    token_provider: Box<dyn TokenProvider>,

    // FIXME: If we test that this object is not Send+Sync, is it safe to
    // assume that the RefCell will never be borrowed and replaced at the
    // same time?
    access_token: RefCell<String>,
}

impl ServerClientImpl {
    pub fn new(
        service_address: ServiceAddress,
        token_provider: Box<dyn TokenProvider>,
    ) -> Result<Self> {
        let access_token = RefCell::new(token_provider.get_access_token()?);
        Ok(Self {
            service_address,
            token_provider,
            access_token,
        })
    }

    pub fn get_configuration(&self, collection: &ConfigurationId) -> Result<ConfigurationJson> {
        let url = format!(
            "{}/feature/v1/instances/{}/config",
            self.service_address.base_url(ServiceAddressProtocol::Http),
            collection.guid
        );
        let client = Client::new();
        let r = client
            .get(url)
            .query(&[
                ("action", "sdkConfig"),
                ("environment_id", &collection.environment_id),
                ("collection_id", &collection.collection_id),
            ])
            .header("Accept", "application/json")
            .header("User-Agent", "appconfiguration-rust-sdk/0.0.1")
            .bearer_auth(self.access_token.borrow())
            .send();

        match r {
            Ok(response) => response.json().map_err(Error::ReqwestError),
            Err(e) => {
                // TODO: Identify if token expired, get new one and retry
                if false {
                    let access_token = self.token_provider.get_access_token()?;
                    self.access_token.replace(access_token);
                }
                Err(e.into())
            }
        }
    }

    pub fn get_configuration_monitoring_websocket(
        &self,
        collection: &ConfigurationId,
    ) -> Result<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
        let ws_url = format!(
            "{}/wsfeature",
            self.service_address.base_url(ServiceAddressProtocol::Ws)
        );
        let mut ws_url = Url::parse(&ws_url)
            .map_err(|e| Error::Other(format!("Cannot parse '{}' as URL: {}", ws_url, e)))?;

        ws_url
            .query_pairs_mut()
            .append_pair("instance_id", &collection.guid)
            .append_pair("environment_id", &collection.environment_id)
            .append_pair("collection_id", &collection.collection_id);

        let mut request = ws_url
            .as_str()
            .into_client_request()
            .map_err(Error::TungsteniteError)?;
        let headers = request.headers_mut();
        headers.insert(
            "User-Agent",
            "appconfiguration-rust-sdk/0.0.1"
                .parse()
                .map_err(|_| Error::Other("Invalid header value for 'User-Agent'".to_string()))?,
        );
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.access_token.borrow())
                .parse()
                .map_err(|_| {
                    Error::Other("Invalid header value for 'Authorization'".to_string())
                })?,
        );

        Ok(connect(request)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_ssl_base_url() {
        let address = ServiceAddress::new_without_ssl(
            "ibm.com".to_string(),
            None,
            Some("endpoint".to_string()),
        );
        assert_eq!(
            address.base_url(ServiceAddressProtocol::Http),
            "http://ibm.com/endpoint"
        );
        assert_eq!(
            address.base_url(ServiceAddressProtocol::Ws),
            "ws://ibm.com/endpoint"
        );
    }

    #[test]
    fn test_ssl_base_url() {
        let address =
            ServiceAddress::new("ibm.com".to_string(), None, Some("endpoint".to_string()));
        assert_eq!(
            address.base_url(ServiceAddressProtocol::Http),
            "https://ibm.com/endpoint"
        );
        assert_eq!(
            address.base_url(ServiceAddressProtocol::Ws),
            "wss://ibm.com/endpoint"
        );
    }

    #[test]
    fn test_url_with_port() {
        let address = ServiceAddress::new_without_ssl("ibm.com".to_string(), Some(12345), None);
        assert_eq!(
            address.base_url(ServiceAddressProtocol::Http),
            "http://ibm.com:12345"
        );
        assert_eq!(
            address.base_url(ServiceAddressProtocol::Ws),
            "ws://ibm.com:12345"
        );
    }
}

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

use super::{NetworkError, NetworkResult, TokenProvider};
use crate::models::Configuration;
use crate::ConfigurationId;
use reqwest::blocking::Client;
use std::sync::Arc;

use tungstenite::client::IntoClientRequest;

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

#[derive(Debug, Clone)]
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

pub(crate) trait WebsocketReader: Send + 'static {
    /// Reads a message from the stream, if possible. If the connection have been closed,
    /// this will also return the close message
    fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message>;
}

impl<T: std::io::Read + std::io::Write + Send + Sync + 'static> WebsocketReader
    for tungstenite::WebSocket<T>
{
    fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
        self.read()
    }
}

pub trait ServerClient: Send + 'static {
    fn get_configuration(&self, configuration_id: &ConfigurationId)
        -> NetworkResult<Configuration>;

    fn get_configuration_monitoring_websocket(
        &self,
        collection: &ConfigurationId,
    ) -> NetworkResult<impl WebsocketReader>;
}

#[derive(Debug)]
pub(crate) struct ServerClientImpl {
    service_address: ServiceAddress,
    token_provider: Arc<Box<dyn TokenProvider>>,
}

impl ServerClientImpl {
    pub fn new(
        service_address: ServiceAddress,
        token_provider: Arc<Box<dyn TokenProvider>>,
    ) -> NetworkResult<Self> {
        Ok(Self {
            service_address,
            token_provider,
        })
    }
}

impl ServerClient for ServerClientImpl {
    fn get_configuration(
        &self,
        configuration_id: &ConfigurationId,
    ) -> NetworkResult<Configuration> {
        let url = format!(
            "{}/feature/v1/instances/{}/config",
            self.service_address.base_url(ServiceAddressProtocol::Http),
            configuration_id.guid
        );
        let url = Url::parse(&url).map_err(|_| NetworkError::UrlParseError(url))?;
        let client = Client::new();
        let r = client
            .get(url)
            .query(&[
                ("action", "sdkConfig"),
                ("environment_id", &configuration_id.environment_id),
                ("collection_id", &configuration_id.collection_id),
            ])
            .header("Accept", "application/json")
            .header("User-Agent", "appconfiguration-rust-sdk/0.0.1")
            .bearer_auth(self.token_provider.get_access_token()?)
            .send();

        match r {
            Ok(response) => {
                // TODO: Handle response.status()
                let config_json = response.json().map_err(|_| NetworkError::ProtocolError)?;
                Ok(Configuration::new(
                    &configuration_id.environment_id,
                    config_json,
                )?)
            }
            Err(e) => {
                // TODO: Identify if token expired, get new one and retry
                // NOTE: bad status might not necessarily map to a reqwest error.
                // For metering there is a test where server returns a bad status (e.g. Token expired).
                // In this test reqwest client returns Ok(response). So if we intend to do token renewal,
                // this Err(e) match arm might not be the right place.
                // if false {
                //     let access_token = self.token_provider.get_access_token()?;
                //     self.access_token.replace(access_token);
                // }
                Err(e.into())
            }
        }
    }

    fn get_configuration_monitoring_websocket(
        &self,
        collection: &ConfigurationId,
    ) -> NetworkResult<impl WebsocketReader> {
        let ws_url = format!(
            "{}/wsfeature",
            self.service_address.base_url(ServiceAddressProtocol::Ws)
        );
        let mut ws_url = Url::parse(&ws_url).map_err(|_| NetworkError::UrlParseError(ws_url))?;

        ws_url
            .query_pairs_mut()
            .append_pair("instance_id", &collection.guid)
            .append_pair("environment_id", &collection.environment_id)
            .append_pair("collection_id", &collection.collection_id);

        let mut request = ws_url
            .as_str()
            .into_client_request()
            .map_err(NetworkError::TungsteniteError)?;
        let headers = request.headers_mut();
        headers.insert(
            "User-Agent",
            "appconfiguration-rust-sdk/0.0.1"
                .parse()
                .map_err(|_| NetworkError::InvalidHeaderValue("User-Agent".to_string()))?,
        );
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.token_provider.get_access_token()?)
                .parse()
                .map_err(|_| NetworkError::InvalidHeaderValue("Authorization".to_string()))?,
        );

        let (websocket, _) = connect(request)?;
        Ok(websocket)
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

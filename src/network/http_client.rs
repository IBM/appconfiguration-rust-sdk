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
use crate::ConfigurationId;
use crate::models::Configuration;
use crate::network::serialization::ConfigurationJson;
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use std::sync::Arc;
use std::time::Duration;

use tungstenite::client::IntoClientRequest;

use tungstenite::connect;
use tungstenite::stream::MaybeTlsStream;
use url::Url;

pub(crate) const SDK_USER_AGENT: &str =
    concat!("appconfiguration-rust-sdk/", env!("CARGO_PKG_VERSION"));
pub(crate) const WEBSOCKET_READ_TIMEOUT_SECS: u64 = 65;
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

pub trait WebsocketReader: Send + 'static {
    /// Reads a message from the stream, if possible. If the connection have been closed,
    /// this will also return the close message
    fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message>;

    // Add a flush method so your handler can force auto-pongs out to the network
    fn flush_socket(&mut self) -> tungstenite::error::Result<()>;
}

impl<T: std::io::Read + std::io::Write + Send + Sync + 'static> WebsocketReader
    for tungstenite::WebSocket<T>
{
    fn read_msg(&mut self) -> tungstenite::error::Result<tungstenite::Message> {
        self.read()
    }

    fn flush_socket(&mut self) -> tungstenite::error::Result<()> {
        self.flush()
    }
}

pub trait ServerClient: Send + 'static {
    #[allow(dead_code)]
    fn get_configuration(&self, configuration_id: &ConfigurationId)
    -> NetworkResult<Configuration>;

    fn get_configuration_monitoring_websocket(
        &self,
        collection: &ConfigurationId,
    ) -> NetworkResult<impl WebsocketReader>;

    fn get_configuration_json(
        &self,
        _configuration_id: &ConfigurationId,
    ) -> NetworkResult<ConfigurationJson> {
        // Default implementation is intentionally unimplemented. Concrete types
        // that fetch raw JSON from a remote server must override this method.
        // Using `unimplemented!()` here instead of a silent error ensures that
        // any implementor that forgets to override this will fail loudly at
        // runtime rather than silently performing double work and returning an
        // error after the network round-trip.
        unimplemented!(
            "get_configuration_json must be overridden by concrete ServerClient implementations"
        )
    }
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

    fn build_http_client() -> NetworkResult<Client> {
        ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(NetworkError::ReqwestError)
    }

    fn build_default_headers(is_post: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));

        if is_post {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }

        headers
    }

    fn build_authorization_header(&self) -> NetworkResult<HeaderValue> {
        let bearer = format!("Bearer {}", self.token_provider.get_access_token()?);
        HeaderValue::from_str(&bearer)
            .map_err(|_| NetworkError::InvalidHeaderValue("Authorization".to_string()))
    }
}

impl ServerClient for ServerClientImpl {
    fn get_configuration(
        &self,
        configuration_id: &ConfigurationId,
    ) -> NetworkResult<Configuration> {
        let config_json = self.get_configuration_json(configuration_id)?;

        Ok(Configuration::new(
            &configuration_id.environment_id,
            &configuration_id.collection_id,
            config_json,
        )?)
    }

    fn get_configuration_json(
        &self,
        configuration_id: &ConfigurationId,
    ) -> NetworkResult<ConfigurationJson> {
        log::debug!("Fetching configuration JSON from server");
        let url = format!(
            "{}/feature/v1/instances/{}/config",
            self.service_address.base_url(ServiceAddressProtocol::Http),
            configuration_id.guid
        );
        let client = Self::build_http_client()?;
        let mut headers = Self::build_default_headers(false);
        headers.insert(AUTHORIZATION, self.build_authorization_header()?);

        client
            .get(url)
            .query(&[
                ("action", "sdkConfig"),
                ("environment_id", &configuration_id.environment_id),
                ("collection_id", &configuration_id.collection_id),
            ])
            .headers(headers)
            .send()
            .map_err(NetworkError::ReqwestError)?
            .error_for_status()
            .map_err(NetworkError::ReqwestError)?
            .json::<ConfigurationJson>()
            .map_err(|_| NetworkError::ProtocolError)
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
        headers.insert(USER_AGENT, HeaderValue::from_static(SDK_USER_AGENT));
        headers.insert(AUTHORIZATION, self.build_authorization_header()?);
        log::debug!(
            "[WEBSOCKET] Establishing WebSocket connection to {}",
            ws_url
        );
        let (mut websocket, response) = connect(request).map_err(|error| match error {
            tungstenite::Error::Http(response) => {
                log::warn!(
                    "[WEBSOCKET] HTTP error during WebSocket handshake: {}",
                    response.status().as_str()
                );
                NetworkError::WebsocketHttpStatus {
                    status_code: response.status().as_u16(),
                    message: response
                        .status()
                        .canonical_reason()
                        .unwrap_or("Unknown websocket HTTP error")
                        .to_string(),
                }
            }
            other => {
                log::warn!("[WEBSOCKET] Connection error: {}", other);
                NetworkError::TungsteniteError(other)
            }
        })?;
        let _ = response;
        let timeout_duration = Duration::from_secs(WEBSOCKET_READ_TIMEOUT_SECS);

        let timeout_result = match websocket.get_mut() {
            MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(timeout_duration)),
            MaybeTlsStream::NativeTls(s) => s.get_mut().set_read_timeout(Some(timeout_duration)),
            _ => {
                log::warn!("Unknown underlying stream type. Read timeout could not be set.");
                Ok(())
            }
        };

        if let Err(e) = timeout_result {
            log::error!("Failed to set TCP read timeout: {:?}", e);
        }
        log::debug!("[WEBSOCKET] Connection established successfully");
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

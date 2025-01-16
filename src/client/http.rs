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

use std::net::TcpStream;

use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Response;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, WebSocket};
use url::Url;

use crate::errors::{Error, Result};

pub fn get_ws_url(region: &str) -> String {
    format!("wss://{region}.apprapp.cloud.ibm.com/apprapp/wsfeature")
}

pub fn get_configuration_monitoring_websocket(
    access_token: &str,
    region: &str,
    guid: &str,
    collection_id: &str,
    environment_id: &str,
) -> Result<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
    let url = get_ws_url(region);
    let mut url = Url::parse(&url)
        .map_err(|e| Error::Other(format!("Cannot parse '{}' as URL: {}", url, e)))?;

    url.query_pairs_mut()
        .append_pair("instance_id", guid)
        .append_pair("collection_id", collection_id)
        .append_pair("environment_id", environment_id);

    let mut request = url
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
        format!("Bearer {}", access_token)
            .parse()
            .map_err(|_| Error::Other("Invalid header value for 'Authorization'".to_string()))?,
    );

    Ok(connect(request)?)
}

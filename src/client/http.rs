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

use std::collections::HashMap;
use std::net::TcpStream;

use reqwest::blocking::Client;
use serde::Deserialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Response;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, WebSocket};
use url::Url;

use crate::errors::{Error, Result};
use crate::models;

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: String,
}

pub fn get_base_url(region: &str, guid: &str) -> String {
    format!("https://{region}.apprapp.cloud.ibm.com/apprapp/feature/v1/instances/{guid}/config")
}

pub fn get_ws_url(region: &str) -> String {
    format!("wss://{region}.apprapp.cloud.ibm.com/apprapp/wsfeature")
}

pub fn get_access_token(apikey: &str) -> Result<String> {
    let mut form_data = HashMap::new();
    form_data.insert("reponse_type".to_string(), "cloud_iam".to_string());
    form_data.insert(
        "grant_type".to_string(),
        "urn:ibm:params:oauth:grant-type:apikey".to_string(),
    );
    form_data.insert("apikey".to_string(), apikey.to_string());

    let client = Client::new();
    Ok(client
        .post("https://iam.cloud.ibm.com/identity/token")
        .header("Accept", "application/json")
        .form(&form_data)
        .send()
        .map_err(Error::ReqwestError)?
        .json::<AccessTokenResponse>()
        .map_err(Error::ReqwestError)? // FIXME: This is a deserialization error (extract it from Reqwest)
        .access_token)
}

pub fn get_configuration(
    access_token: &str,
    region: &str,
    guid: &str,
    collection_id: &str,
    environment_id: &str,
) -> Result<models::Configuration> {
    let client = Client::new();
    let url = get_base_url(region, guid);
    client
        .get(&url)
        .query(&[
            ("action", "sdkConfig"),
            ("collection_id", collection_id),
            ("environment_id", environment_id),
        ])
        .header("Accept", "application/json")
        .header("User-Agent", "appconfiguration-rust-sdk/0.0.1")
        .bearer_auth(access_token)
        .send()
        .map_err(Error::ReqwestError)?
        .json()
        .map_err(Error::ReqwestError) // FIXME: This is a deserialization error (extract it from Reqwest)
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

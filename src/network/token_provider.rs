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

use reqwest::blocking::Client;
use serde::Deserialize;

use super::{NetworkError, NetworkResult};

pub trait TokenProvider: std::fmt::Debug + Send + Sync {
    fn get_access_token(&self) -> NetworkResult<String>;
}

#[derive(Debug)]
pub(crate) struct IBMCloudTokenProvider {
    apikey: String,
}

impl IBMCloudTokenProvider {
    pub fn new(apikey: &str) -> Self {
        Self {
            apikey: apikey.to_string(),
        }
    }
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: String,
}

impl TokenProvider for IBMCloudTokenProvider {
    fn get_access_token(&self) -> NetworkResult<String> {
        let mut form_data = HashMap::new();
        form_data.insert("reponse_type".to_string(), "cloud_iam".to_string());
        form_data.insert(
            "grant_type".to_string(),
            "urn:ibm:params:oauth:grant-type:apikey".to_string(),
        );
        form_data.insert("apikey".to_string(), self.apikey.to_string());

        let client = Client::new();
        Ok(client
            .post("https://iam.cloud.ibm.com/identity/token")
            .header("Accept", "application/json")
            .form(&form_data)
            .send()
            .map_err(NetworkError::ReqwestError)?
            .json::<AccessTokenResponse>()
            .map_err(NetworkError::ReqwestError)? // FIXME: This is a deserialization error (extract it from Reqwest)
            .access_token)
    }
}

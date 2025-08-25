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

use std::{cmp::max, collections::HashMap, sync::RwLock};

use super::{NetworkError, NetworkResult};
use reqwest::blocking::Client;
use serde::Deserialize;

pub trait TokenProvider: std::fmt::Debug + Send + Sync {
    fn get_access_token(&self) -> NetworkResult<String>;
}

#[derive(Debug, Default)]
struct AccessToken {
    token: String,
    expiration: u64,
}

impl AccessToken {
    fn expired(&self) -> bool {
        let now = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
        now >= self.expiration
    }

    fn renew(&mut self, token: String, expires_in: u64) -> u64 {
        self.token = token;
        self.expiration = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() + expires_in;
        self.expiration
    }
}

#[derive(Debug)]
pub(crate) struct IBMCloudTokenProvider {
    apikey: String,
    access_token: RwLock<AccessToken>,
}

impl IBMCloudTokenProvider {
    pub fn new(apikey: &str) -> Self {
        Self {
            apikey: apikey.to_string(),
            access_token: RwLock::default(),
        }
    }

    fn expired(&self) -> bool {
        self.access_token.read().map_or(true, |t| t.expired())
    }

    fn safe_expires_in(expires_in: u64) -> u64 {
        if expires_in == 0 {
            expires_in
        } else {
            max((expires_in as f32 * 0.9f32) as u64, 1u64)
        }
    }

    // Renews the stored token.
    //
    // It will use 90% of the expires_in value returned by the server
    // to account for possible latencies and delays between token renewal and usage.
    //
    // It returns new expiration time (unix time)
    fn renew_token(&self) -> NetworkResult<u64> {
        let mut form_data = HashMap::new();
        form_data.insert("reponse_type".to_string(), "cloud_iam".to_string());
        form_data.insert(
            "grant_type".to_string(),
            "urn:ibm:params:oauth:grant-type:apikey".to_string(),
        );
        form_data.insert("apikey".to_string(), self.apikey.to_string());

        let client = Client::new();
        let new_token = client
            .post("https://iam.cloud.ibm.com/identity/token")
            .header("Accept", "application/json")
            .form(&form_data)
            .send()
            .map_err(NetworkError::ReqwestError)?
            .json::<AccessTokenResponse>()
            .map_err(NetworkError::ReqwestError)?; // FIXME: This is a deserialization error (extract it from Reqwest)

        let mut access_token = self.access_token.write()?;
        Ok(access_token.renew(
            new_token.access_token,
            IBMCloudTokenProvider::safe_expires_in(new_token.expires_in),
        ))
    }
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: String,
    expires_in: u64,
    // We are discarding some fields:
    // refresh_token: String, // "not_supported"
    // token_type: String,    // "Bearer"
    // expiration: u64,       // 1755854106 <- unix time when it expires
    // scope: String,         // "ibm openid"
}

impl TokenProvider for IBMCloudTokenProvider {
    fn get_access_token(&self) -> NetworkResult<String> {
        if self.expired() {
            self.renew_token()?;
        }

        Ok(self.access_token.read()?.token.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_token() {
        let mut access_token = AccessToken::default();
        assert!(access_token.expired());

        access_token.renew("something".to_string(), 10);
        assert!(!access_token.expired());
        assert_eq!(access_token.token, "something".to_string());

        // If it expires in zero seconds, it's actually expired already
        access_token.renew("token".to_string(), 0);
        assert!(access_token.expired());
    }

    #[test]
    fn test_ibm_cloud_token_provider() {
        let provider = IBMCloudTokenProvider::new("apikey");
        assert!(provider.expired());

        // If the token is expired, it will try to renew it when requesting it
        assert!(matches!(
            provider.get_access_token().unwrap_err(),
            NetworkError::ReqwestError(_)
        ));

        // If it has not expired, it will just return the token
        provider
            .access_token
            .write()
            .unwrap()
            .renew("the-token".to_string(), 10);
        assert_eq!(
            provider.get_access_token().unwrap(),
            "the-token".to_string()
        );
    }

    #[test]
    fn test_safe_expires_in() {
        assert_eq!(IBMCloudTokenProvider::safe_expires_in(10), 9);
        assert_eq!(IBMCloudTokenProvider::safe_expires_in(2), 1);

        // Corner cases
        assert_eq!(IBMCloudTokenProvider::safe_expires_in(0), 0);
        assert_eq!(IBMCloudTokenProvider::safe_expires_in(1), 1);
    }
}

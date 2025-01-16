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
use crate::models::Configuration;
use crate::{Error, Result};
use reqwest::blocking::Client;
use std::cell::RefCell;

#[derive(Debug)]
pub(crate) struct ServerClientImpl {
    token_provider: Box<dyn TokenProvider>,

    // FIXME: If we test that this object is not Send+Sync, is it safe to
    // assume that the RefCell will never be borrowed and replaced at the
    // same time?
    access_token: RefCell<String>,
}

impl ServerClientImpl {
    pub fn new(token_provider: Box<dyn TokenProvider>) -> Result<Self> {
        let access_token = RefCell::new(token_provider.get_access_token()?);
        Ok(Self {
            token_provider,
            access_token,
        })
    }

    // TODO: To be removed
    pub fn get_access_token(&self) -> String {
        self.access_token.borrow().clone()
    }

    pub fn get_configuration(
        &self,
        url: &str,
        collection_id: &str,
        environment_id: &str,
    ) -> Result<Configuration> {
        let client = Client::new();
        let r = client
            .get(url)
            .query(&[
                ("action", "sdkConfig"),
                ("collection_id", collection_id),
                ("environment_id", environment_id),
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
}

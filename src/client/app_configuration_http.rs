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

use crate::client::cache::ConfigurationSnapshot;
pub use crate::client::feature_proxy::FeatureProxy;
use crate::client::feature_snapshot::FeatureSnapshot;
use crate::client::http;
pub use crate::client::property_proxy::PropertyProxy;
use crate::client::property_snapshot::PropertySnapshot;
use crate::errors::{ConfigurationAccessError, Error, Result};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;
use tungstenite::WebSocket;

use super::TokenProvider;

/// AppConfiguration client connection to IBM Cloud.
#[derive(Debug)]
pub struct AppConfigurationClientHttp {
    // pub(crate) latest_config_snapshot: Arc<Mutex<ConfigurationSnapshot>>,
    // pub(crate) _thread_terminator: std::sync::mpsc::Sender<()>,
}

pub struct ServiceAddress {
    host: String,
    port: Option<u16>,
    endpoint: Option<String>,
}

impl AppConfigurationClientHttp {
    pub fn new(
        service_address: ServiceAddress,
        token_provider: TokenProvider,
        environment_id: &str,
    ) -> Self {
        // TODO: Establish connection and start _update_ thread
        // let access_token = token_provider.get_token();

        Self {}
    }

    fn get_configuration_snapshot(
        access_token: &str,
        // region: &str,
        guid: &str,
        environment_id: &str,
        collection_id: &str,
    ) -> Result<ConfigurationSnapshot> {
        let configuration = http::get_configuration(
            // TODO: access_token might expire. This will cause issues with long-running apps
            access_token,
            region,
            guid,
            collection_id,
            environment_id,
        )?;
        ConfigurationSnapshot::new(environment_id, configuration)
    }
}

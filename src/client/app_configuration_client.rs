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

use std::sync::{Arc, Mutex};

use crate::client::feature_proxy::FeatureProxy;
use crate::client::property_proxy::PropertyProxy;
use crate::models::{FeatureSnapshot, PropertySnapshot, SecretPropertySnapshot};
use crate::network::live_configuration::CurrentModeOfflineReason;
use crate::Result;
/// Identifies a configuration
#[derive(Debug, Clone)]
pub struct ConfigurationId {
    /// Instance ID of the App Configuration service. Obtain it from the service credentials section of the App Configuration dashboard
    pub guid: String,
    /// ID of the environment created in App Configuration service instance under the Environments section.
    pub environment_id: String,
    /// ID of the collection created in App Configuration service instance under the Collections section
    pub collection_id: String,
}

impl ConfigurationId {
    pub fn new(guid: String, environment_id: String, collection_id: String) -> Self {
        Self {
            guid,
            environment_id,
            collection_id,
        }
    }
}

pub trait ConfigurationProvider {
    /// Returns the list of features.
    ///
    /// The list contains the `id`s that can be used in other methods to return
    /// concrete features, like [`get_feature`](appconfiguration-rust-sdk/src/client/app_configuration_client.rs:52).
    fn get_feature_ids(&self) -> Result<Vec<String>>;

    /// Returns a snapshot for a [`Feature`](crate::Feature).
    ///
    /// The instance contains a snapshot with all the values and rules, so it
    /// will always evaluate the same entities to the same values, no updates
    /// will be received from the server.
    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot>;

    /// Returns the list of properties.
    ///
    /// The list contains the `id`s that can be used in other methods to return
    /// concrete properties, like [`get_property`](appconfiguration-rust-sdk/src/client/app_configuration_client.rs:65).
    fn get_property_ids(&self) -> Result<Vec<String>>;

    /// Returns a snapshot for a [`Property`](crate::Property).
    ///
    /// The instance contains a snapshot with all the values and rules, so it
    /// will always evaluate the same entities to the same values, no updates
    /// will be received from the server
    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot>;

    /// Returns a snapshot for a secret-reference property.
    ///
    /// This mirrors the Node SDK [`getSecret()`](appconfiguration-node-sdk/lib/configurations/ConfigurationHandler.js:543)
    /// behavior by returning a dedicated wrapper that can later resolve the final
    /// secret using a user-provided secret manager.
    fn get_secret_property(&self, property_id: &str) -> Result<SecretPropertySnapshot>;
    

    /// Returns whether the live runtime currently has an active server connection.
    ///
    /// Node parity:
    /// this is the Rust-facing equivalent of [`isConnected`](appconfiguration-node-sdk/lib/AppConfiguration.js:314),
    /// while preserving the existing [`is_online()`](appconfiguration-rust-sdk/src/client/app_configuration_client.rs:76)
    /// surface for backwards compatibility.
    fn is_connected(&self) -> Result<bool> {
        self.is_online()
    }

    /// For remote configurations, it returns whether it's connected to the
    /// remote or not
    fn is_online(&self) -> Result<bool>;

    /// Returns the current live runtime state when supported by the implementation.
    fn get_runtime_status(&self) -> Result<Option<RuntimeStatus>> {
        Ok(None)
    }

    /// Registers a listener for live runtime events when supported by the implementation.
    ///
    /// Node parity:
    /// this is the Rust-facing replacement for listening to the public emitter exposed by
    /// [`AppConfiguration`](appconfiguration-node-sdk/lib/AppConfiguration.js:25).
    fn add_runtime_event_listener(&self, _listener: RuntimeEventListener) -> Result<()> {
        Ok(())
    }

    /// For remote configurations: Blocks until connected to the remote.
    fn wait_until_online(&self);

    /// Stops live runtime activity and resets any in-memory orchestration state.
    ///
    /// Node parity note:
    /// this aligns with explicit runtime cleanup expectations around
    /// [`cleanup()`](appconfiguration-node-sdk/lib/configurations/ConfigurationHandler.js:113),
    /// but cache-file deletion behavior is controlled by each implementation.
    fn cleanup(&mut self) -> Result<()>;

    /// Stops live runtime activity, resets orchestration state, and deletes any
    /// SDK-managed persistent cache when supported by the implementation.
    fn cleanup_with_cache_clear(&mut self) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub is_connected: bool,
    pub mode: Option<RuntimeMode>,
    pub offline_reason: Option<CurrentModeOfflineReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMode {
    Online,
    Offline,
    Defunct,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEventKind {
    Connected,
    Disconnected,
    Closed,
    HeartbeatTimeout,
    RefreshSuccess,
    RefreshFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub kind: RuntimeEventKind,
    pub status: RuntimeStatus,
}

pub type RuntimeEventListener = Arc<dyn Fn(RuntimeEvent) + Send + Sync + 'static>;

#[derive(Default, Clone)]
pub struct RuntimeEventEmitter {
    listeners: Arc<Mutex<Vec<RuntimeEventListener>>>,
}

impl RuntimeEventEmitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on(&self, listener: RuntimeEventListener) -> Result<()> {
        self.listeners.lock()?.push(listener);
        Ok(())
    }

    pub fn emit(&self, event: RuntimeEvent) -> Result<()> {
        let listeners = self.listeners.lock()?.clone();

        for listener in listeners {
            listener(event.clone());
        }
        Ok(())
    }
}

/// AppConfiguration client for browsing, and evaluating features and properties.
pub trait AppConfigurationClient: ConfigurationProvider {
    /// Returns a proxied [`Feature`](crate::Feature).
    ///
    /// This proxied feature will envaluate entities using the latest information
    /// available if the client implementation support some kind of live-updates.
    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>>;

    /// Returns a proxied [`Property`](crate::Property).
    ///
    /// This proxied property will envaluate entities using the latest information
    /// available if the client implementation support some kind of live-updates.
    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy<'_>>;

    /// Records a custom experiment/event metric for an entity.
    ///
    /// This is API groundwork toward Node SDK parity with [`track()`](appconfiguration-node-sdk/lib/configurations/ConfigurationHandler.js:716).
    /// The current Rust implementation validates the inputs and exposes the public
    /// surface, but does not yet send experiment metric events.
    fn track(&self, event_key: &str, entity_id: &str) -> Result<()>;
}

impl<T: ConfigurationProvider> AppConfigurationClient for T {
    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>> {
        let _ = self.get_feature(feature_id)?;
        Ok(FeatureProxy::new(self, feature_id.to_string()))
    }

    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy<'_>> {
        let _ = self.get_property(property_id)?;
        Ok(PropertyProxy::new(self, property_id.to_string()))
    }

    fn track(&self, event_key: &str, entity_id: &str) -> Result<()> {
        if event_key.is_empty() || entity_id.is_empty() {
            return Err(crate::Error::Other(
                "event_key or entity_id cannot be empty".to_string(),
            ));
        }

        Err(crate::Error::Other(
            "track() is not implemented yet in the Rust SDK".to_string(),
        ))
    }
}

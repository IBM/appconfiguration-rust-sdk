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
use crate::Result;
use crate::client::feature_proxy::FeatureProxy;
use crate::client::property_proxy::PropertyProxy;
use crate::models::{FeatureSnapshot, PropertySnapshot, SecretPropertySnapshot};
use crate::network::live_configuration::CurrentModeOfflineReason;
use std::sync::{Arc, Mutex};
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
    /// concrete features, like [`get_feature`](ConfigurationProvider::get_feature).
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
    /// concrete properties, like [`get_property`](ConfigurationProvider::get_property).
    fn get_property_ids(&self) -> Result<Vec<String>>;

    /// Returns a snapshot for a [`Property`](crate::Property).
    ///
    /// The instance contains a snapshot with all the values and rules, so it
    /// will always evaluate the same entities to the same values, no updates
    /// will be received from the server
    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot>;

    /// For remote configurations, it returns whether it's connected to the
    /// remote or not
    fn is_online(&self) -> Result<bool>;

    /// For remote configurations: Blocks until connected to the remote.
    ///
    /// Returns `true` if the client came online within the timeout window,
    /// `false` if the timeout elapsed before a connection was established.
    fn wait_until_online(&self) -> bool;

    fn get_secret_property(&self, property_id: &str) -> Result<SecretPropertySnapshot>;

    fn is_connected(&self) -> Result<bool> {
        self.is_online()
    }

    fn get_runtime_status(&self) -> Result<Option<RuntimeStatus>> {
        Ok(None)
    }

    fn add_runtime_event_listener(&self, _listener: RuntimeEventListener) -> Result<()> {
        Ok(())
    }

    fn clean_up(&mut self) -> Result<()> {
        Ok(())
    }

    fn clean_up_with_cache_clear(&mut self) -> Result<()> {
        Ok(())
    }
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
    /// This proxied feature will evaluate entities using the latest information
    /// available if the client implementation supports some kind of live-updates.
    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>>;

    /// Returns a proxied [`Property`](crate::Property).
    ///
    /// This proxied property will evaluate entities using the latest information
    /// available if the client implementation supports some kind of live-updates.
    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy<'_>>;

    /// Track a custom event for analytics.
    ///
    /// **Note:** This feature is not yet implemented. The default implementation
    /// is a deliberate no-op that returns `Ok(())` to avoid breaking downstream
    /// trait implementations. It will be replaced with a real implementation
    /// in a future release.
    fn track(&self, _event_key: &str, _entity_id: &str) -> Result<()> {
        Ok(())
    }
}

impl<T: ConfigurationProvider> AppConfigurationClient for T {
    fn get_feature_proxy<'a>(&'a self, feature_id: &str) -> Result<FeatureProxy<'a>> {
        // Do NOT eagerly probe get_feature() here: when the client is temporarily
        // offline the probe fails with a connectivity error (Offline(WebsocketError))
        // that callers misread as "feature not found".  The proxy itself defers all
        // reads to call time, so errors surface there with proper context.
        Ok(FeatureProxy::new(self, feature_id.to_string()))
    }

    fn get_property_proxy(&self, property_id: &str) -> Result<PropertyProxy<'_>> {
        // Same rationale as get_feature_proxy: no eager probe.
        Ok(PropertyProxy::new(self, property_id.to_string()))
    }
}

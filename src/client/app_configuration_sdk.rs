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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::client::{
    AppConfigurationClientIBMCloud, ConfigurationId, ConfigurationProvider, RuntimeEvent,
    RuntimeEventEmitter, RuntimeStatus,
};
use crate::errors::{Error, Result};
use crate::models::{FeatureSnapshot, PropertySnapshot, SecretManager, SecretPropertySnapshot};
use crate::OfflineMode;

/**
 * Node-style top-level SDK wrapper that separates credential initialization
 * from configuration context binding.
 *
 * This mirrors the high-level flow of [`AppConfiguration.js`](../../../../appconfiguration-node-sdk/lib/AppConfiguration.js),
 * where callers:
 * 1. create an SDK wrapper,
 * 2. optionally configure private-endpoint behavior,
 * 3. call [`AppConfigurationSdk::init()`],
 * 4. call [`AppConfigurationSdk::set_context()`],
 * 5. evaluate features and properties through the [`ConfigurationProvider`] API.
 */
#[derive(Default)]
pub struct AppConfigurationSdk {
    init_state: Option<InitState>,
    client: Option<AppConfigurationClientIBMCloud>,
    runtime_emitter: RuntimeEventEmitter,
}

#[derive(Debug, Clone)]
struct InitState {
    apikey: String,
    region: String,
    guid: String,
    use_private_endpoint: bool,
}

/**
 * Context options used by [`AppConfigurationSdk::set_context()`].
 *
 * These fields intentionally mirror the Node top-level `setContext(..., options)` shape:
 * - [`persistent_cache_directory`](appconfiguration-rust-sdk/src/client/app_configuration_sdk.rs:50)
 * - [`bootstrap_file`](appconfiguration-rust-sdk/src/client/app_configuration_sdk.rs:51)
 * - [`live_config_update_enabled`](appconfiguration-rust-sdk/src/client/app_configuration_sdk.rs:52)
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfigurationContextOptions {
    pub persistent_cache_directory: Option<PathBuf>,
    pub bootstrap_file: Option<PathBuf>,
    pub live_config_update_enabled: bool,
}

impl Default for AppConfigurationContextOptions {
    fn default() -> Self {
        Self {
            persistent_cache_directory: None,
            bootstrap_file: None,
            live_config_update_enabled: true,
        }
    }
}

impl AppConfigurationContextOptions {
    pub fn validate(&self) -> Result<()> {
        if let Some(path) = &self.persistent_cache_directory {
            validate_non_empty_path(
                path,
                "persistent_cache_directory cannot be empty when provided",
            )?;
        }

        if let Some(path) = &self.bootstrap_file {
            validate_non_empty_path(path, "bootstrap_file cannot be empty when provided")?;

            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                return Err(Error::Other(format!(
                    "bootstrap_file must point to a .json file, got '{}'",
                    path.display()
                )));
            }
        }

        if !self.live_config_update_enabled && self.bootstrap_file.is_none() {
            return Err(Error::Other(
                "live_config_update_enabled=false requires bootstrap_file".to_string(),
            ));
        }

        Ok(())
    }
}

impl AppConfigurationSdk {
    /// Creates a new top-level SDK wrapper.
    ///
    /// Typical usage is:
    /// - call [`AppConfigurationSdk::new()`]
    /// - optionally call [`AppConfigurationSdk::use_private_endpoint()`]
    /// - call [`AppConfigurationSdk::init()`]
    /// - call [`AppConfigurationSdk::set_context()`]
    pub fn new() -> Self {
        Self::default()
    }

    /// Controls whether the SDK should use the IBM Cloud private endpoint.
    ///
    /// Like the Node SDK, this is intended to be called before
    /// [`AppConfigurationSdk::init()`].
    pub fn use_private_endpoint(&mut self, use_private_endpoint: bool) {
        match self.init_state.as_mut() {
            Some(state) => state.use_private_endpoint = use_private_endpoint,
            None => {
                self.init_state = Some(InitState {
                    apikey: String::new(),
                    region: String::new(),
                    guid: String::new(),
                    use_private_endpoint,
                });
            }
        }
    }

    /// Initializes the SDK with service credentials.
    ///
    /// Repeated calls are ignored after the first successful initialization,
    /// matching the Node SDK's one-time init behavior.
    pub fn init(&mut self, region: &str, guid: &str, apikey: &str) -> Result<()> {
        if self.is_initialized() {
            return Ok(());
        }

        validate_required("region", region)?;
        validate_required("guid", guid)?;
        validate_required("apikey", apikey)?;

        let use_private_endpoint = self
            .init_state
            .as_ref()
            .map(|state| state.use_private_endpoint)
            .unwrap_or(false);

        self.init_state = Some(InitState {
            apikey: apikey.to_string(),
            region: region.to_string(),
            guid: guid.to_string(),
            use_private_endpoint,
        });

        Ok(())
    }

    /// Binds the SDK to a collection/environment context and constructs the live client.
    ///
    /// Repeated calls are ignored after the context has already been set,
    /// matching the Node SDK's one-time `setContext()` behavior.
    pub fn set_context(
        &mut self,
        collection_id: &str,
        environment_id: &str,
        options: AppConfigurationContextOptions,
    ) -> Result<()> {
        if self.client.is_some() {
            return Ok(());
        }

        let init_state = self.init_state.clone().ok_or_else(|| {
            Error::Other("init must be called before set_context".to_string())
        })?;

        validate_required("collection_id", collection_id)?;
        validate_required("environment_id", environment_id)?;
        options.validate()?;

        let offline_mode = build_offline_mode(&options, environment_id, collection_id);
        let configuration_id = ConfigurationId::new(
            init_state.guid,
            environment_id.to_string(),
            collection_id.to_string(),
        );

        let client = AppConfigurationClientIBMCloud::new(
            &init_state.apikey,
            &init_state.region,
            configuration_id,
            offline_mode,
            init_state.use_private_endpoint,
        )?;

        let runtime_emitter = self.runtime_emitter.clone();
        client.add_runtime_event_listener(Arc::new(move |event: RuntimeEvent| {
            runtime_emitter.emit(event);
        }))?;
        self.client = Some(client);
        Ok(())
    }

    /// Returns whether [`AppConfigurationSdk::init()`] has been completed.
    pub fn is_initialized(&self) -> bool {
        self.init_state
            .as_ref()
            .map(|state| {
                !state.region.is_empty() && !state.guid.is_empty() && !state.apikey.is_empty()
            })
            .unwrap_or(false)
    }

    /// Returns whether [`AppConfigurationSdk::set_context()`] has been completed.
    pub fn is_context_set(&self) -> bool {
        self.client.is_some()
    }

    fn client(&self) -> Result<&AppConfigurationClientIBMCloud> {
        self.client
            .as_ref()
            .ok_or(Error::ClientNotConfigured)
    }
    /// Resolves a secret-reference property using the provided entity and secret manager.
    ///
    /// This mirrors the high-level Node SDK [`getSecret()`](appconfiguration-node-sdk/lib/AppConfiguration.js:260)
    /// convenience flow from the top-level SDK wrapper, while still allowing lower-level access
    /// through [`ConfigurationProvider::get_secret_property()`](appconfiguration-rust-sdk/src/client/app_configuration_client.rs:75).
    pub fn get_secret(
        &self,
        property_id: &str,
        entity: &impl crate::Entity,
        secret_manager: &impl SecretManager,
    ) -> Result<String> {
        self.client()?
            .get_secret_property(property_id)?
            .get_current_value(entity, secret_manager)
    }

    /// Returns a reusable emitter-style runtime event surface similar to the public Node SDK emitter.
    pub fn emitter(&self) -> RuntimeEventEmitter {
        self.runtime_emitter.clone()
    }
}

impl ConfigurationProvider for AppConfigurationSdk {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        self.client()?.get_feature_ids()
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        self.client()?.get_feature(feature_id)
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        self.client()?.get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        self.client()?.get_property(property_id)
    }

    fn get_secret_property(&self, property_id: &str) -> Result<SecretPropertySnapshot> {
        self.client()?.get_secret_property(property_id)
    }

    fn is_connected(&self) -> Result<bool> {
        self.client()?.is_connected()
    }

    fn is_online(&self) -> Result<bool> {
        self.client()?.is_online()
    }

    fn get_runtime_status(&self) -> Result<Option<RuntimeStatus>> {
        self.client()?.get_runtime_status()
    }

    fn wait_until_online(&self) {
        if let Some(client) = self.client.as_ref() {
            client.wait_until_online();
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        if let Some(client) = self.client.as_mut() {
            client.cleanup()?;
        }
        self.client = None;
        Ok(())
    }

    fn cleanup_with_cache_clear(&mut self) -> Result<()> {
        if let Some(client) = self.client.as_mut() {
            client.cleanup_with_cache_clear()?;
        }
        self.client = None;
        Ok(())
    }
}

fn validate_required(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Other(format!("{name} is required")));
    }

    Ok(())
}

fn validate_non_empty_path(path: &Path, error_message: &str) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::Other(error_message.to_string()));
    }

    Ok(())
}

fn build_offline_mode(
    options: &AppConfigurationContextOptions,
    environment_id: &str,
    collection_id: &str,
) -> OfflineMode {
    if let Some(bootstrap_file) = &options.bootstrap_file {
        return OfflineMode::bootstrap_file(
            bootstrap_file,
            environment_id.to_string(),
            collection_id.to_string(),
        );
    }

    if let Some(persistent_cache_directory) = &options.persistent_cache_directory {
        let cache_file = persistent_cache_directory.join("appconfiguration.json");
        return OfflineMode::persistent_cache_file(
            cache_file,
            environment_id.to_string(),
            collection_id.to_string(),
        );
    }

    if options.live_config_update_enabled {
        OfflineMode::Fail
    } else {
        OfflineMode::Fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_require_json_bootstrap() {
        let err = AppConfigurationContextOptions {
            bootstrap_file: Some(PathBuf::from("config.txt")),
            ..Default::default()
        }
        .validate()
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "bootstrap_file must point to a .json file, got 'config.txt'"
        );
    }

    #[test]
    fn options_require_bootstrap_for_non_live_mode() {
        let err = AppConfigurationContextOptions {
            live_config_update_enabled: false,
            ..Default::default()
        }
        .validate()
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "live_config_update_enabled=false requires bootstrap_file"
        );
    }

    #[test]
    fn init_is_required_before_set_context() {
        let mut sdk = AppConfigurationSdk::new();

        let err = sdk
            .set_context(
                "collection",
                "environment",
                AppConfigurationContextOptions::default(),
            )
            .unwrap_err();

        assert_eq!(err.to_string(), "init must be called before set_context");
    }

    #[test]
    fn init_is_only_applied_once() {
        let mut sdk = AppConfigurationSdk::new();

        sdk.init("us-south", "guid-1", "apikey-1").unwrap();
        sdk.init("eu-de", "guid-2", "apikey-2").unwrap();

        let init_state = sdk.init_state.unwrap();
        assert_eq!(init_state.region, "us-south");
        assert_eq!(init_state.guid, "guid-1");
        assert_eq!(init_state.apikey, "apikey-1");
    }

    #[test]
    fn private_endpoint_flag_survives_until_init() {
        let mut sdk = AppConfigurationSdk::new();
        sdk.use_private_endpoint(true);

        sdk.init("us-south", "guid-1", "apikey-1").unwrap();

        assert!(sdk.init_state.unwrap().use_private_endpoint);
    }

    #[test]
    fn offline_mode_mapping_matches_options() {
        let options = AppConfigurationContextOptions {
            persistent_cache_directory: Some(PathBuf::from("/tmp/cache")),
            bootstrap_file: Some(PathBuf::from("/tmp/bootstrap.json")),
            live_config_update_enabled: true,
        };

        match build_offline_mode(&options, "environment", "collection") {
            OfflineMode::BootstrapFile {
                path,
                environment_id,
                collection_id,
            } => {
                assert_eq!(path, PathBuf::from("/tmp/bootstrap.json"));
                assert_eq!(environment_id, "environment");
                assert_eq!(collection_id, "collection");
            }
            other => panic!("unexpected offline mode: {:?}", other),
        }
    }
}

// Made with Bob

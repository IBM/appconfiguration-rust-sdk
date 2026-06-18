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

use crate::OfflineMode;
use crate::client::app_configuration_ibm_cloud::{
    ResolvedUrls, resolve_urls_from_service_override,
};
use crate::client::app_configuration_offline::AppConfigurationOffline;
use crate::client::{
    AppConfigurationClientIBMCloud, ConfigurationId, ConfigurationProvider, RuntimeEventEmitter,
    RuntimeStatus,
};
use crate::errors::{Error, Result};
use crate::models::{FeatureSnapshot, PropertySnapshot, SecretManager, SecretPropertySnapshot};

#[derive(Default)]
pub struct AppConfiguration {
    init_state: Option<InitState>,
    client: Option<AppConfigurationClientIBMCloud>,
    runtime_emitter: RuntimeEventEmitter,
    /// Set by [`AppConfiguration::override_service_url`] before `init()`.
    service_url_override: Option<String>,
}

#[derive(Debug, Clone)]
struct InitState {
    apikey: String,
    region: String,
    guid: String,
    use_private_endpoint: bool,
    /// Pre-resolved URLs computed at `init()` time from `service_url_override`.
    resolved_urls: ResolvedUrls,
}

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
    /// Constructs and immediately validates the options, returning an error if
    /// the configuration is inconsistent (e.g. `live_config_update_enabled=false`
    /// without a `bootstrap_file`). Prefer this over constructing the struct
    /// directly to catch misconfiguration at construction time.
    pub fn try_new(
        persistent_cache_directory: Option<PathBuf>,
        bootstrap_file: Option<PathBuf>,
        live_config_update_enabled: bool,
    ) -> Result<Self> {
        let opts = Self {
            persistent_cache_directory,
            bootstrap_file,
            live_config_update_enabled,
        };
        opts.validate()?;
        Ok(opts)
    }

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

impl AppConfiguration {
    /// Typical usage is:
    /// - call [`AppConfiguration::new()`]
    /// - optionally call [`AppConfiguration::override_service_url()`]
    /// - optionally call [`AppConfiguration::use_private_endpoint()`]
    /// - call [`AppConfiguration::init()`]
    /// - call [`AppConfiguration::set_context()`]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the App Configuration service URL.
    ///
    /// Must be called **before** [`AppConfiguration::init()`].
    ///
    /// This is primarily useful for:
    /// - Connecting to the IBM Cloud **test/staging** environment, e.g.
    ///   `"https://dndev.apprapp.test.cloud.ibm.com"`.
    ///   The SDK automatically selects `iam.test.cloud.ibm.com` as the IAM
    ///   token endpoint whenever the host contains `test.cloud.ibm.com`.
    /// - Local development / mock servers, e.g. `"http://localhost:3000"`.
    ///   In that case the IAM token endpoint falls back to
    ///   `iam.cloud.ibm.com` (production).
    ///
    /// ```ignore
    /// sdk.override_service_url("https://dndev.apprapp.test.cloud.ibm.com");
    /// sdk.init(region, guid, apikey)?;
    /// ```
    pub fn override_service_url(&mut self, url: &str) {
        self.service_url_override = Some(url.to_string());
    }

    /// Controls whether the SDK should use the IBM Cloud private endpoint.
    pub fn use_private_endpoint(&mut self, use_private_endpoint: bool) {
        match self.init_state.as_mut() {
            Some(state) => state.use_private_endpoint = use_private_endpoint,
            None => {
                self.init_state = Some(InitState {
                    apikey: String::new(),
                    region: String::new(),
                    guid: String::new(),
                    use_private_endpoint,
                    resolved_urls: ResolvedUrls::default(),
                });
            }
        }
    }

    /// Initializes the SDK with service credentials.
    ///
    /// Repeated calls are ignored after the first successful initialization.
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

        // Resolve URLs once at init time so set_context() can just pass them through.
        let resolved_urls = match &self.service_url_override {
            Some(url) => resolve_urls_from_service_override(url, use_private_endpoint),
            None => ResolvedUrls::default(),
        };

        self.init_state = Some(InitState {
            apikey: apikey.to_string(),
            region: region.to_string(),
            guid: guid.to_string(),
            use_private_endpoint,
            resolved_urls,
        });

        Ok(())
    }

    /// Binds the SDK to a collection/environment context and constructs the live client.
    ///
    /// Repeated calls are ignored after the context has already been set,
    pub fn set_context(
        &mut self,
        collection_id: &str,
        environment_id: &str,
        options: AppConfigurationContextOptions,
    ) -> Result<()> {
        if self.client.is_some() {
            return Ok(());
        }

        let init_state = self
            .init_state
            .clone()
            .ok_or_else(|| Error::Other("init must be called before set_context".to_string()))?;

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
            init_state.resolved_urls,
            self.runtime_emitter.clone(),
        )?;

        self.client = Some(client);
        Ok(())
    }

    /// Returns whether [`AppConfiguration::init()`] has been completed.
    pub fn is_initialized(&self) -> bool {
        self.init_state
            .as_ref()
            .map(|state| {
                !state.region.is_empty() && !state.guid.is_empty() && !state.apikey.is_empty()
            })
            .unwrap_or(false)
    }

    /// Returns whether [`AppConfiguration::set_context()`] has been completed.
    pub fn is_context_set(&self) -> bool {
        self.client.is_some()
    }

    fn client(&self) -> Result<&AppConfigurationClientIBMCloud> {
        self.client.as_ref().ok_or(Error::ClientNotConfigured)
    }
    /// Resolves a secret-reference property using the provided entity and secret manager.
    ///
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

    pub fn emitter(&self) -> RuntimeEventEmitter {
        self.runtime_emitter.clone()
    }
}

impl ConfigurationProvider for AppConfiguration {
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

    fn wait_until_online(&self) -> bool {
        if let Some(client) = self.client.as_ref() {
            client.wait_until_online()
        } else {
            false
        }
    }

    fn clean_up(&mut self) -> Result<()> {
        if let Some(client) = self.client.as_mut() {
            client.clean_up()?;
        }
        self.client = None;
        Ok(())
    }

    fn clean_up_with_cache_clear(&mut self) -> Result<()> {
        if let Some(client) = self.client.as_mut() {
            client.clean_up_with_cache_clear()?;
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
        if !options.live_config_update_enabled {
            match AppConfigurationOffline::new(bootstrap_file, environment_id, collection_id) {
                Ok(offline) => return OfflineMode::FallbackData(offline),
                Err(e) => {
                    log::warn!(
                        "Failed to load bootstrap file, falling back to BootstrapFile mode: {e}"
                    );
                }
            }
        }
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
        // live_config_update_enabled=false requires bootstrap_file (validated in validate()),
        // so the BootstrapFile branch above will have already returned. This branch is only
        // reached if validation was bypassed — default to Cache as a safe fallback.
        OfflineMode::Cache
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
        let mut sdk = AppConfiguration::new();

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
        let mut sdk = AppConfiguration::new();

        sdk.init("us-south", "guid-1", "apikey-1").unwrap();
        sdk.init("eu-de", "guid-2", "apikey-2").unwrap();

        let init_state = sdk.init_state.unwrap();
        assert_eq!(init_state.region, "us-south");
        assert_eq!(init_state.guid, "guid-1");
        assert_eq!(init_state.apikey, "apikey-1");
    }

    #[test]
    fn private_endpoint_flag_survives_until_init() {
        let mut sdk = AppConfiguration::new();
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

    // ── override_service_url ──────────────────────────────────────────────────

    /// When no override is set the resolved_urls must be the default (all None).
    #[test]
    fn no_override_produces_default_resolved_urls() {
        let mut sdk = AppConfiguration::new();
        sdk.init("us-south", "guid-1", "apikey-1").unwrap();

        let state = sdk.init_state.unwrap();
        assert!(state.resolved_urls.service_host_override.is_none());
        assert!(state.resolved_urls.token_url_override.is_none());
    }

    /// override_service_url with test.cloud.ibm.com must auto-select the test IAM.
    #[test]
    fn override_service_url_test_domain_selects_test_iam() {
        let mut sdk = AppConfiguration::new();
        sdk.override_service_url("https://dndev.apprapp.test.cloud.ibm.com");
        sdk.init("us-south", "guid-1", "apikey-1").unwrap();

        let state = sdk.init_state.unwrap();
        assert_eq!(
            state.resolved_urls.service_host_override.as_deref(),
            Some("dndev.apprapp.test.cloud.ibm.com")
        );
        assert_eq!(
            state.resolved_urls.token_url_override.as_deref(),
            Some("https://iam.test.cloud.ibm.com/identity/token")
        );
    }

    /// override_service_url with a production cloud.ibm.com URL must keep prod IAM.
    #[test]
    fn override_service_url_prod_domain_keeps_prod_iam() {
        let mut sdk = AppConfiguration::new();
        sdk.override_service_url("https://custom.apprapp.cloud.ibm.com");
        sdk.init("us-south", "guid-1", "apikey-1").unwrap();

        let state = sdk.init_state.unwrap();
        assert_eq!(
            state.resolved_urls.token_url_override.as_deref(),
            Some("https://iam.cloud.ibm.com/identity/token")
        );
    }

    /// override_service_url must be called before init; a second init is a no-op.
    #[test]
    fn override_service_url_must_be_before_init() {
        let mut sdk = AppConfiguration::new();
        sdk.init("us-south", "guid-1", "apikey-1").unwrap();

        // override_service_url after init doesn't affect the already-initialized state.
        sdk.override_service_url("https://dndev.apprapp.test.cloud.ibm.com");
        // Second init is ignored.
        sdk.init("eu-de", "guid-2", "apikey-2").unwrap();

        let state = sdk.init_state.unwrap();
        // First init's data is preserved.
        assert_eq!(state.region, "us-south");
        // No URL override was baked in (it was set after the first init).
        assert!(state.resolved_urls.service_host_override.is_none());
    }

    /// override_service_url with localhost leaves SSL disabled.
    #[test]
    fn override_service_url_localhost_no_ssl() {
        let mut sdk = AppConfiguration::new();
        sdk.override_service_url("http://localhost:8080");
        sdk.init("us-south", "guid-1", "apikey-1").unwrap();

        let state = sdk.init_state.unwrap();
        assert!(state.resolved_urls.service_no_ssl);
        assert_eq!(state.resolved_urls.service_port_override, Some(8080));
    }
}

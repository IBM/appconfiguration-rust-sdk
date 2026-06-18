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
// TODO : Check this implementation of prod and test url.
use crate::errors::Result;
use crate::models::{FeatureSnapshot, PropertySnapshot, SecretManager, SecretPropertySnapshot};
use crate::network::ServiceAddress;
use crate::network::live_configuration::LiveConfigurationImpl;
use crate::{ConfigurationProvider, OfflineMode, RuntimeEventEmitter, TokenProviderImpl};

use super::{ConfigurationId, RuntimeEventListener, RuntimeStatus};
use crate::client::app_configuration_http::AppConfigurationClientHttp;

// ── IAM hostname constants ────────────────────────────────────────────────────

const IAM_PROD_HOST: &str = "iam.cloud.ibm.com";
const IAM_TEST_HOST: &str = "iam.test.cloud.ibm.com";

/// Resolved URL configuration passed from [`AppConfiguration`] down to this
/// client.  All fields are pre-computed strings so the lower layers have no
/// decision-making to do.
#[derive(Debug, Clone, Default)]
pub struct ResolvedUrls {
    /// Fully-qualified service host (no scheme, no path).
    /// `None` → derive from region + private-endpoint flag + domain.
    pub service_host_override: Option<String>,
    /// Full IAM token URL (including scheme and path).
    /// `None` → derive from private-endpoint flag + domain.
    pub token_url_override: Option<String>,
    /// When `true` the service host override came without TLS (e.g. `http://`).
    pub service_no_ssl: bool,
    /// Optional port parsed from the service URL override.
    pub service_port_override: Option<u16>,
}

/// AppConfiguration client connection to IBM Cloud.
#[derive(Debug)]
pub struct AppConfigurationClientIBMCloud {
    client: AppConfigurationClientHttp<LiveConfigurationImpl>,
}

impl AppConfigurationClientIBMCloud {
    /// Creates a new client connecting to IBM Cloud.
    ///
    /// This client keeps a WebSocket open to the server to receive live-updates
    /// to features and properties.
    ///
    /// # Arguments
    ///
    /// * `apikey`              – IAM API key.
    /// * `region`              – Region where the service instance lives.
    /// * `configuration_id`    – Collection / environment to use.
    /// * `offline_mode`        – Behaviour when not synced with the server.
    /// * `use_private_endpoint`– Use IBM Cloud private network endpoint.
    /// * `resolved_urls`       – Pre-computed URL overrides (see
    ///                           [`AppConfiguration::override_service_url`]).
    pub fn new(
        apikey: &str,
        region: &str,
        configuration_id: ConfigurationId,
        offline_mode: OfflineMode,
        use_private_endpoint: bool,
        resolved_urls: ResolvedUrls,
        runtime_emitter: RuntimeEventEmitter,
    ) -> Result<Self> {
        let service_address =
            Self::build_service_address(region, use_private_endpoint, &resolved_urls);
        let token_provider = Box::new(Self::build_token_provider(
            apikey,
            use_private_endpoint,
            &resolved_urls,
        ));

        Ok(Self {
            client: AppConfigurationClientHttp::new(
                service_address,
                token_provider,
                configuration_id,
                offline_mode,
                runtime_emitter,
            )?,
        })
    }

    // ── Internal URL builders ────────────────────────────────────────────────

    pub(crate) fn build_service_address(
        region: &str,
        use_private_endpoint: bool,
        urls: &ResolvedUrls,
    ) -> ServiceAddress {
        if let Some(host) = &urls.service_host_override {
            if urls.service_no_ssl {
                return ServiceAddress::new_without_ssl(
                    host.clone(),
                    urls.service_port_override,
                    Some("apprapp".to_string()),
                );
            }
            return ServiceAddress::new(
                host.clone(),
                urls.service_port_override,
                Some("apprapp".to_string()),
            );
        }

        // Default: production cloud.ibm.com
        let host = if use_private_endpoint {
            format!("private.{region}.apprapp.cloud.ibm.com")
        } else {
            format!("{region}.apprapp.cloud.ibm.com")
        };
        ServiceAddress::new(host, None, Some("apprapp".to_string()))
    }

    pub(crate) fn build_token_provider(
        apikey: &str,
        use_private_endpoint: bool,
        urls: &ResolvedUrls,
    ) -> TokenProviderImpl {
        if let Some(token_url) = &urls.token_url_override {
            return TokenProviderImpl::new(apikey, token_url);
        }

        // Default: production IAM
        let host = if use_private_endpoint {
            format!("private.{IAM_PROD_HOST}")
        } else {
            IAM_PROD_HOST.to_string()
        };
        TokenProviderImpl::new(apikey, &format!("https://{host}/identity/token"))
    }

    // ── Public helpers ───────────────────────────────────────────────────────

    pub fn get_secret(
        &self,
        property_id: &str,
        entity: &impl crate::Entity,
        secret_manager: &impl SecretManager,
    ) -> Result<String> {
        self.client
            .get_secret_property(property_id)?
            .get_current_value(entity, secret_manager)
    }
}

/// Parses a raw `override_service_url` string like
/// `"https://dndev.apprapp.test.cloud.ibm.com"` or `"http://localhost:3000"`
/// and produces a [`ResolvedUrls`] with the token URL automatically derived
/// when the host belongs to `test.cloud.ibm.com`.
///
/// Rules:
/// - URL contains `test.cloud.ibm.com` → token from `iam.test.cloud.ibm.com`
/// - Any other URL                      → token from `iam.cloud.ibm.com` (production)
///
/// The `use_private_endpoint` flag is still respected for the IAM host when no
/// explicit token URL is given.
pub fn resolve_urls_from_service_override(
    raw_url: &str,
    use_private_endpoint: bool,
) -> ResolvedUrls {
    let use_ssl = raw_url.starts_with("https://") || raw_url.starts_with("wss://");

    // Strip scheme.
    let without_scheme = raw_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");

    // Split authority from path (drop path – the SDK always appends /apprapp).
    let authority = without_scheme
        .split_once('/')
        .map(|(auth, _)| auth)
        .unwrap_or(without_scheme);

    // Split host from optional port.
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        // Make sure the left side isn't an IPv6 address without port.
        if let Ok(p) = p.parse::<u16>() {
            (h.to_string(), Some(p))
        } else {
            (authority.to_string(), None)
        }
    } else {
        (authority.to_string(), None)
    };

    // Derive the IAM token URL from the service host.
    let iam_base = if host.contains("test.cloud.ibm.com") {
        if use_private_endpoint {
            format!("private.{IAM_TEST_HOST}")
        } else {
            IAM_TEST_HOST.to_string()
        }
    } else {
        if use_private_endpoint {
            format!("private.{IAM_PROD_HOST}")
        } else {
            IAM_PROD_HOST.to_string()
        }
    };

    ResolvedUrls {
        service_host_override: Some(host),
        token_url_override: Some(format!("https://{iam_base}/identity/token")),
        service_no_ssl: !use_ssl,
        service_port_override: port,
    }
}

impl ConfigurationProvider for AppConfigurationClientIBMCloud {
    fn get_feature_ids(&self) -> Result<Vec<String>> {
        self.client.get_feature_ids()
    }

    fn get_feature(&self, feature_id: &str) -> Result<FeatureSnapshot> {
        self.client.get_feature(feature_id)
    }

    fn get_property_ids(&self) -> Result<Vec<String>> {
        self.client.get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> Result<PropertySnapshot> {
        self.client.get_property(property_id)
    }

    fn is_online(&self) -> Result<bool> {
        self.client.is_online()
    }

    fn wait_until_online(&self) -> bool {
        self.client.wait_until_online()
    }

    fn get_secret_property(&self, property_id: &str) -> Result<SecretPropertySnapshot> {
        self.client.get_secret_property(property_id)
    }

    fn is_connected(&self) -> Result<bool> {
        self.client.is_connected()
    }

    fn get_runtime_status(&self) -> Result<Option<RuntimeStatus>> {
        self.client.get_runtime_status()
    }

    fn add_runtime_event_listener(&self, listener: RuntimeEventListener) -> Result<()> {
        self.client.add_runtime_event_listener(listener)
    }

    fn clean_up(&mut self) -> Result<()> {
        self.client.clean_up()
    }

    fn clean_up_with_cache_clear(&mut self) -> Result<()> {
        self.client.clean_up_with_cache_clear()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::http_client::ServiceAddressProtocol;

    // Helper: production defaults (no override)
    fn no_override() -> ResolvedUrls {
        ResolvedUrls::default()
    }

    // ── Production defaults ───────────────────────────────────────────────────

    #[test]
    fn test_default_service_address_public() {
        let sa = AppConfigurationClientIBMCloud::build_service_address(
            "us-south",
            false,
            &no_override(),
        );
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Http),
            "https://us-south.apprapp.cloud.ibm.com/apprapp"
        );
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Ws),
            "wss://us-south.apprapp.cloud.ibm.com/apprapp"
        );
    }

    #[test]
    fn test_default_service_address_private() {
        let sa =
            AppConfigurationClientIBMCloud::build_service_address("eu-de", true, &no_override());
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Http),
            "https://private.eu-de.apprapp.cloud.ibm.com/apprapp"
        );
    }

    #[test]
    fn test_default_token_url_public() {
        assert_eq!(
            AppConfigurationClientIBMCloud::build_token_provider("key", false, &no_override())
                .endpoint,
            "https://iam.cloud.ibm.com/identity/token"
        );
    }

    #[test]
    fn test_default_token_url_private() {
        assert_eq!(
            AppConfigurationClientIBMCloud::build_token_provider("key", true, &no_override())
                .endpoint,
            "https://private.iam.cloud.ibm.com/identity/token"
        );
    }

    #[test]
    fn test_override_dndev_service_url_and_auto_token() {
        let urls =
            resolve_urls_from_service_override("https://dndev.apprapp.test.cloud.ibm.com", false);

        let sa = AppConfigurationClientIBMCloud::build_service_address("us-south", false, &urls);
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Http),
            "https://dndev.apprapp.test.cloud.ibm.com/apprapp"
        );
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Ws),
            "wss://dndev.apprapp.test.cloud.ibm.com/apprapp"
        );

        // Token must automatically come from iam.test.cloud.ibm.com
        let tp = AppConfigurationClientIBMCloud::build_token_provider("key", false, &urls);
        assert_eq!(tp.endpoint, "https://iam.test.cloud.ibm.com/identity/token");
    }

    #[test]
    fn test_override_test_domain_private_endpoint_token() {
        // Private endpoint on the test domain
        let urls =
            resolve_urls_from_service_override("https://dndev.apprapp.test.cloud.ibm.com", true);
        let tp = AppConfigurationClientIBMCloud::build_token_provider("key", true, &urls);
        assert_eq!(
            tp.endpoint,
            "https://private.iam.test.cloud.ibm.com/identity/token"
        );
    }

    #[test]
    fn test_override_prod_domain_keeps_prod_token() {
        // Overriding the URL but still pointing at cloud.ibm.com → prod IAM
        let urls =
            resolve_urls_from_service_override("https://custom.apprapp.cloud.ibm.com", false);
        let tp = AppConfigurationClientIBMCloud::build_token_provider("key", false, &urls);
        assert_eq!(tp.endpoint, "https://iam.cloud.ibm.com/identity/token");
    }

    // ── Arbitrary dev / localhost override ────────────────────────────────────

    #[test]
    fn test_override_localhost_with_port() {
        let urls = resolve_urls_from_service_override("http://localhost:3000", false);

        let sa = AppConfigurationClientIBMCloud::build_service_address("us-south", false, &urls);
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Http),
            "http://localhost:3000/apprapp"
        );
        // Localhost is not *.test.cloud.ibm.com → falls back to prod IAM
        let tp = AppConfigurationClientIBMCloud::build_token_provider("key", false, &urls);
        assert_eq!(tp.endpoint, "https://iam.cloud.ibm.com/identity/token");
    }

    #[test]
    fn test_override_https_no_port() {
        let urls = resolve_urls_from_service_override("https://my-mock.example.com", false);
        let sa = AppConfigurationClientIBMCloud::build_service_address("us-south", false, &urls);
        assert_eq!(
            sa.base_url(ServiceAddressProtocol::Http),
            "https://my-mock.example.com/apprapp"
        );
    }

    // ── Region is still respected when no override is present ─────────────────

    #[test]
    fn test_region_used_when_no_override() {
        for region in &["us-south", "eu-de", "au-syd", "jp-tok"] {
            let sa = AppConfigurationClientIBMCloud::build_service_address(
                region,
                false,
                &no_override(),
            );
            assert!(
                sa.base_url(ServiceAddressProtocol::Http).contains(region),
                "host should contain region '{region}'"
            );
        }
    }
}

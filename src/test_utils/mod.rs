use crate::client::app_configuration_http::AppConfigurationClientHttp;
use crate::network::{NetworkResult, ServiceAddress, TokenProvider};
use crate::{AppConfigurationClient, Result};
use crate::{ConfigurationId, OfflineMode};
#[derive(Debug)]
struct MockTokenProvider {}

impl TokenProvider for MockTokenProvider {
    fn get_access_token(&self) -> NetworkResult<String> {
        Ok("mock_token".into())
    }
}

/// Creates and returns an [`AppConfigurationClient`]-like object that connects to
/// the given server.
pub fn create_app_configuration_client_live(
    service_address: ServiceAddress,
    configuration_id: ConfigurationId,
    offline_mode: OfflineMode,
) -> Result<Box<dyn AppConfigurationClient>> {
    let token_provider = Box::new(MockTokenProvider {});
    let client = AppConfigurationClientHttp::new(
        service_address,
        token_provider,
        configuration_id,
        offline_mode,
    )?;

    Ok(Box::new(client))
}

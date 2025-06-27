use appconfiguration::{AppConfigurationOffline, ConfigurationId, OfflineMode, ServiceAddress};

use std::net::TcpListener;

use std::path::PathBuf;

mod common;

#[test]
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Bind a random port, so we are sure that there is no server there (client will never connect).
    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();

    let client = {
        // Offline data
        let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");
        let offline_data = AppConfigurationOffline::new(&mocked_data, "dev")?;

        // The actual client
        let address = ServiceAddress::new_without_ssl(
            "127.0.0.1".to_string(),
            Some(port),
            Some("test".to_string()),
        );
        let config_id = ConfigurationId::new(
            "guid".to_string(),
            "dev".to_string(),
            "collection_id".to_string(),
        );

        appconfiguration::test_utils::create_app_configuration_client_live(
            address,
            config_id,
            OfflineMode::FallbackData(offline_data),
        )
        .unwrap()
    };

    // The client is not online
    assert!(!client.is_online().unwrap());

    // but it retrieves the fallback data
    let mut features = client.get_feature_ids().unwrap();
    features.sort();
    assert_eq!(features, vec!["f1", "f2", "f3", "f4", "f5", "f6"]);

    Ok(())
}

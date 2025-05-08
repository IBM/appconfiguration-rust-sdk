use appconfiguration::{
    AppConfigurationClientHttp, ConfigurationId, ConfigurationProvider, OfflineMode, ServiceAddress,
};

use std::net::TcpListener;
use std::sync::mpsc::channel;
use std::thread::{sleep, spawn};
use std::time::Duration;

mod common;

struct ServerHandle {
    _terminator: std::sync::mpsc::Sender<()>,
    config_updated: std::sync::mpsc::Sender<()>,
    port: u16,
}
fn server_thread() -> ServerHandle {
    let (terminator, receiver) = channel();
    let (update_config_tx, update_config_rx) = channel();

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();
    spawn(move || {
        // notify client that config changed
        let mut websocket = common::handle_websocket(&server);

        common::handle_config_request_enterprise_example(&server);

        // Wait until the client has already tested the first configuration
        update_config_rx.recv().unwrap();

        // Notify there is new configuration
        websocket
            .send(tungstenite::Message::text(
                "notify config changed".to_string(),
            ))
            .unwrap();

        // client will request changed config asynchronously
        common::handle_config_request_trivial_config(&server);

        let _ = receiver.recv();
    });
    ServerHandle {
        _terminator: terminator,
        config_updated: update_config_tx,
        port,
    }
}

#[test]
fn main() {
    let server = server_thread();

    let address = ServiceAddress::new_without_ssl(
        "127.0.0.1".to_string(),
        Some(server.port),
        Some("test".to_string()),
    );
    let config_id = ConfigurationId::new(
        "guid".to_string(),
        "dev".to_string(),
        "collection_id".to_string(),
    );
    let client = AppConfigurationClientHttp::new(
        address,
        Box::new(common::MockTokenProvider {}),
        config_id,
        OfflineMode::Fail,
    )
    .unwrap();

    common::wait_until_online(&client);

    let mut features = client.get_feature_ids().unwrap();
    features.sort();
    assert_eq!(features, vec!["f1", "f2", "f3", "f4", "f5", "f6"]);

    // Tell the server that now it can actually send the new config
    server.config_updated.send(()).unwrap();

    let start = std::time::Instant::now();
    loop {
        // We need the loop for now to wait until client updates the config.
        let features = client.get_feature_ids().unwrap();
        if features.is_empty() {
            // This is what we expect after the config update :)
            break;
        }
        if start.elapsed().as_millis() > 1000 {
            panic!("Did not receive updated configuration in time")
        }
        sleep(Duration::from_millis(10));
    }
}

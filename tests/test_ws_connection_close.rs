use appconfiguration::{ConfigurationId, OfflineMode, ServiceAddress};

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
    let (config_updated_tx, update_config_rx) = channel();

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();
    spawn(move || {
        // notify client that config changed
        let mut websocket = common::handle_websocket(&server);

        common::handle_config_request_enterprise_example(&server);

        // Wait for the client to recieve (and test) the first config
        update_config_rx.recv().unwrap();

        // Now send a WS close message. Server goes away!
        websocket.send(tungstenite::Message::Close(None)).unwrap();

        let _ = receiver.recv();
    });
    ServerHandle {
        _terminator: terminator,
        config_updated: config_updated_tx,
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
    let client = appconfiguration::test_utils::create_app_configuration_client_live(
        address,
        config_id,
        OfflineMode::Fail,
    )
    .unwrap();

    common::wait_until_online(&client);

    // Tell the server that now it can progress
    server.config_updated.send(()).unwrap();
    sleep(Duration::from_millis(10));

    // Close the WS on the server side
    let r = client.get_feature("id");
    assert!(r.is_err(), "{:?}", r);
    assert_eq!(
        r.unwrap_err().to_string(),
        "Connection to server lost: WebsocketClosed"
    );

    // We are not online
    let r = client.is_online();
    assert!(matches!(r, Ok(false)));

    // Clean-up: when `server` goes out of scope, it will destroy it's `_terminator` and
    // the server thread will be killed. When it goes away, the thread in the client
    // will enter a loop trying to reconnect to the server and failing because there is no
    // server listening at the URL... however, it will retry and retry because it's considered
    // a recoverable error (in prod, server might become alive again).
    // Finally, when the test session ends, the thread will be garbage-collected.
}

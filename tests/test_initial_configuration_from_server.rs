use appconfiguration::{
    AppConfigurationClient, AppConfigurationClientHttp, ConfigurationId, LiveConfigurationImpl,
    ServiceAddress, TokenProvider,
};

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::thread::{sleep, spawn};
use std::time::Duration;
use tungstenite::WebSocket;

fn handle_config_request_trivial_config(server: &TcpListener) {
    let json_payload = serde_json::json!({
        "environments": [
            {
                "name": "Dev",
                "environment_id": "dev",
                "features": [],
                "properties": []
            }
        ],
        "segments": []
    });
    handle_config_request(server, json_payload.to_string());
}

fn handle_config_request_enterprise_example(server: &TcpListener) {
    let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");
    let json_payload = std::fs::read_to_string(mocked_data).unwrap();

    handle_config_request(server, json_payload);
}

fn handle_config_request(server: &TcpListener, json_payload: String) {
    let (mut stream, _) = server.accept().unwrap();

    let buf_reader = BufReader::new(&stream);
    let http_request: Vec<_> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(http_request[0], "GET /test/feature/v1/instances/guid/config?action=sdkConfig&environment_id=dev&collection_id=collection_id HTTP/1.1");

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        json_payload.len(),
        json_payload
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn handle_websocket(server: &TcpListener) -> WebSocket<TcpStream> {
    let (stream, _) = server.accept().unwrap();
    let mut websocket = tungstenite::accept(stream).unwrap();
    websocket
        .send(tungstenite::Message::text("test message".to_string()))
        .unwrap();
    websocket
}

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
        let mut websocket = handle_websocket(&server);

        handle_config_request_enterprise_example(&server);

        // Wait until the client has already tested the first configuration
        update_config_rx.recv().unwrap();

        // Notify there is new configuration
        websocket
            .send(tungstenite::Message::text(
                "notify config changed".to_string(),
            ))
            .unwrap();

        // client will request changed config asynchronously
        handle_config_request_trivial_config(&server);

        let _ = receiver.recv();
    });
    ServerHandle {
        _terminator: terminator,
        config_updated: update_config_tx,
        port,
    }
}

#[derive(Debug)]
struct MockTokenProvider {}

impl TokenProvider for MockTokenProvider {
    fn get_access_token(&self) -> appconfiguration::NetworkResult<String> {
        Ok("mock_token".into())
    }
}

fn wait_until_online(client: &AppConfigurationClientHttp<LiveConfigurationImpl>) {
    loop {
        if client.is_online().unwrap() {
            break;
        };
        sleep(Duration::from_millis(10));
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
    let client =
        AppConfigurationClientHttp::new(address, Box::new(MockTokenProvider {}), config_id)
            .unwrap();

    wait_until_online(&client);

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

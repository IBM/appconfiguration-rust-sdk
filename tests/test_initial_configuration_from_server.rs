use std::{
    io::{BufRead, Write},
    net::{TcpListener, TcpStream},
    thread::{sleep, spawn},
    time::Duration,
};

use appconfiguration::{
    AppConfigurationClient, AppConfigurationClientHttp, ConfigurationId, ServiceAddress,
    TokenProvider,
};

use std::io::BufReader;
use std::sync::mpsc::channel;
use std::{fs, path::PathBuf};
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
    let json_payload = fs::read_to_string(mocked_data).unwrap();

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
        .send(tungstenite::Message::text("test messag".to_string()))
        .unwrap();
    websocket
}

struct ServerHandle {
    _terminator: std::sync::mpsc::Sender<()>,
    config_updated: std::sync::mpsc::Receiver<()>,
    port: u16,
}
fn server_thread() -> ServerHandle {
    let (terminator, receiver) = channel();
    let (config_updated_tx, config_updated_rx) = channel();

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();
    spawn(move || {
        handle_config_request_enterprise_example(&server);

        // notify client that config changed
        let _websocket = handle_websocket(&server);

        // client will request changed config asynchronously
        handle_config_request_trivial_config(&server);

        // we now allow the test to continue
        config_updated_tx.send(()).unwrap();

        let _ = receiver.recv();
    });
    ServerHandle {
        _terminator: terminator,
        config_updated: config_updated_rx,
        port,
    }
}

#[derive(Debug)]
struct MockTokenProvider {}

impl TokenProvider for MockTokenProvider {
    fn get_access_token(&self) -> appconfiguration::Result<String> {
        Ok("mock_token".into())
    }
}

#[test]
fn main() {
    let server = server_thread();

    sleep(Duration::from_secs(1));
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

    let mut features = client.get_feature_ids().unwrap();
    features.sort();
    assert_eq!(features, vec!["f1", "f2", "f3", "f4", "f5", "f6"]);

    // TODO: Once we can subscribe to config updates via client APIs, we don't need to wait for signal, neither the loop below
    server.config_updated.recv().unwrap();

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

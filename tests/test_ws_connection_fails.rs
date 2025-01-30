use appconfiguration::{
    AppConfigurationClientHttp, ConfigurationId, Error, ServiceAddress, TokenProvider,
};

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::thread::spawn;

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

fn handle_websocket_error(server: &TcpListener) {
    let (mut stream, _) = server.accept().unwrap();
    stream.write_all(b"not today").unwrap();
}

struct ServerHandle {
    _terminator: std::sync::mpsc::Sender<()>,
    port: u16,
}
fn server_thread() -> ServerHandle {
    let (terminator, receiver) = channel();

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();
    spawn(move || {
        handle_config_request_enterprise_example(&server);

        // notify client that config changed
        handle_websocket_error(&server);

        let _ = receiver.recv();
    });
    ServerHandle {
        _terminator: terminator,
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
        AppConfigurationClientHttp::new(address, Box::new(MockTokenProvider {}), config_id);

    assert!(client.is_err());
    assert!(matches!(client.unwrap_err(), Error::TungsteniteError(_)));
}

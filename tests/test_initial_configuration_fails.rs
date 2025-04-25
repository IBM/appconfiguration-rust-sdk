use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc::channel;
use std::thread::spawn;

use appconfiguration::{
    AppConfigurationClientHttp, ConfigurationId, NetworkError, ServiceAddress, TokenProvider,
};

fn handle_config_request_error(server: &TcpListener) {
    let (mut stream, _) = server.accept().unwrap();

    let buf_reader = BufReader::new(&stream);
    let http_request: Vec<_> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(http_request[0], "GET /test/feature/v1/instances/guid/config?action=sdkConfig&environment_id=dev&collection_id=collection_id HTTP/1.1");
    stream
        .write_all(b"HTTP/1.1 400\r\nContent-Length: 0")
        .unwrap();
}

fn handle_config_request_invalid_json(server: &TcpListener) {
    let json_payload = serde_json::json!({
        "environments": [
            {
                "name": "Dev",
                "environment_id": "dev",
                "features": ["models deserialization mismatch"],
                "properties": []
            }
        ],
        "segments": []
    })
    .to_string();

    let (mut stream, _) = server.accept().unwrap();

    let buf_reader = BufReader::new(&stream);
    let http_request: Vec<_> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(http_request[0], "GET /test/feature/v1/instances/guid/config?action=sdkConfig&environment_id=dev&collection_id=collection_id HTTP/1.1");
    stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                json_payload.len(),
                json_payload
            )
            .as_bytes(),
        )
        .unwrap();
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
        handle_config_request_error(&server);

        handle_config_request_invalid_json(&server);

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
    fn get_access_token(&self) -> appconfiguration::NetworkResult<String> {
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

    // Test response error code 400
    let client = AppConfigurationClientHttp::new(
        address.clone(),
        Box::new(MockTokenProvider {}),
        config_id.clone(),
    );

    assert!(client.is_err());
    assert!(matches!(
        client.unwrap_err(),
        appconfiguration::Error::NetworkError(NetworkError::ReqwestError(_))
    ));

    // Test response is successful (200) but configuration JSON is invalid
    let client =
        AppConfigurationClientHttp::new(address, Box::new(MockTokenProvider {}), config_id);

    assert!(client.is_err());
    assert!(matches!(
        client.unwrap_err(),
        appconfiguration::Error::NetworkError(NetworkError::ProtocolError)
    ));
}

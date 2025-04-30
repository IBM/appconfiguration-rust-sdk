use appconfiguration::{
    AppConfigurationClient, AppConfigurationClientHttp, ConfigurationId, LiveConfigurationImpl,
    ServiceAddress, TokenProvider,
};
use tungstenite::WebSocket;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::thread::{sleep, spawn};
use std::time::Duration;

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
    let (config_updated_tx, update_config_rx) = channel();

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();
    spawn(move || {
        // notify client that config changed
        let mut websocket = handle_websocket(&server);

        handle_config_request_enterprise_example(&server);

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

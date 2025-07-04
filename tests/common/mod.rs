use appconfiguration::AppConfigurationClient;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use std::thread::sleep;
use std::time::Duration;
use tungstenite::WebSocket;

pub fn handle_config_request_trivial_config(server: &TcpListener) {
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

pub fn handle_config_request_enterprise_example(server: &TcpListener) {
    let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");
    let json_payload = std::fs::read_to_string(mocked_data).unwrap();

    handle_config_request(server, json_payload);
}

pub fn handle_config_request(server: &TcpListener, json_payload: String) {
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

pub fn handle_websocket(server: &TcpListener) -> WebSocket<TcpStream> {
    let (stream, _) = server.accept().unwrap();
    let mut websocket = tungstenite::accept(stream).unwrap();
    websocket
        .send(tungstenite::Message::text("test message".to_string()))
        .unwrap();
    websocket
}

pub fn wait_until_online(client: &Box<dyn AppConfigurationClient>) {
    loop {
        if client.is_online().unwrap() {
            break;
        };
        sleep(Duration::from_millis(10));
    }
}

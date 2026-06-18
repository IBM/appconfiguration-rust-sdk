// Copyright 2026 IBM Corp. All Rights Reserved.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

//       http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use ibm_appconfiguration_rust_sdk::{ConfigurationId, OfflineMode, ServiceAddress};

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
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
        eprintln!("Server thread started, waiting for websocket connection...");
        // Accept first connection - this will be the websocket
        let (stream, _) = server.accept().unwrap();
        eprintln!("Websocket connection accepted");
        let mut websocket = tungstenite::accept(stream).unwrap();
        websocket
            .send(tungstenite::Message::text("test message".to_string()))
            .unwrap();

        eprintln!("Waiting for HTTP config request...");
        // Accept second connection - this will be the HTTP config request
        let (mut stream, _) = server.accept().unwrap();
        eprintln!("HTTP config request accepted");
        let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");
        let json_payload = std::fs::read_to_string(mocked_data).unwrap();

        {
            let buf_reader = BufReader::new(&stream);
            let _http_request: Vec<_> = buf_reader
                .lines()
                .map(|result| result.unwrap())
                .take_while(|line| !line.is_empty())
                .collect();
        }

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json_payload.len(),
            json_payload
        );
        eprintln!("Sending HTTP response, {} bytes", response.len());
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        drop(stream); // Close the connection
        eprintln!("HTTP response sent successfully and connection closed");

        // Wait until the client has already tested the first configuration
        update_config_rx.recv().unwrap();

        // Notify there is new configuration
        websocket
            .send(tungstenite::Message::text(
                "notify config changed".to_string(),
            ))
            .unwrap();

        // client will request changed config asynchronously
        // Accept third connection - updated config request
        let (mut stream, _) = server.accept().unwrap();
        let json_payload = serde_json::json!({
            "environments": [
                {
                    "name": "Dev",
                    "environment_id": "dev",
                    "features": [],
                    "properties": []
                }
            ],
            "segments": [],
            "collections": [
                {
                    "collection_id": "blue-charge",
                    "name": "Blue Charge"
                }
            ]
        });

        {
            let buf_reader = BufReader::new(&stream);
            let _http_request: Vec<_> = buf_reader
                .lines()
                .map(|result| result.unwrap())
                .take_while(|line| !line.is_empty())
                .collect();
        }

        let json_str = json_payload.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json_str.len(),
            json_str
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        drop(stream); // Close the connection

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
        "blue-charge".to_string(),
    );
    let client = ibm_appconfiguration_rust_sdk::test_utils::create_app_configuration_client_live(
        address,
        config_id,
        OfflineMode::Fail,
    )
    .unwrap();

    client.wait_until_online();

    let mut features = client.get_feature_ids().unwrap();
    features.sort();
    assert_eq!(features, vec!["f1", "f2", "f3", "f4", "f6"]);

    // Tell the server that now it can actually send the new config
    server.config_updated.send(()).unwrap();

    let start = std::time::Instant::now();
    let timeout_ms = 10_000u128;
    loop {
        // We need the loop for now to wait until client updates the config.
        let features = client.get_feature_ids().unwrap();
        if features.is_empty() {
            // This is what we expect after the config update :)
            break;
        }
        if start.elapsed().as_millis() > timeout_ms {
            panic!("Did not receive updated configuration in time")
        }
        sleep(Duration::from_millis(10));
    }
}

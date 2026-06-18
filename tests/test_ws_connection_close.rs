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
    let (config_updated_tx, update_config_rx) = channel();

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("Failed to bind");
    let port = server.local_addr().unwrap().port();
    spawn(move || {
        // notify client that config changed
        let (stream, _) = server.accept().unwrap();
        let mut websocket = tungstenite::accept(stream).unwrap();
        websocket
            .send(tungstenite::Message::text("test message".to_string()))
            .unwrap();

        // Accept second connection - this will be the HTTP config request
        let (mut stream, _) = server.accept().unwrap();
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
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        drop(stream); // Close the connection

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
        "blue-charge".to_string(),
    );
    let client = ibm_appconfiguration_rust_sdk::test_utils::create_app_configuration_client_live(
        address,
        config_id,
        OfflineMode::Fail,
    )
    .unwrap();

    client.wait_until_online();

    // Tell the server that now it can progress (send WS close)
    server.config_updated.send(()).unwrap();
    sleep(Duration::from_millis(10));

    // After the WS is closed the SDK transitions to Offline mode, but it must still
    // serve the last successfully-fetched configuration from its in-memory cache
    let r = client.get_feature("f1");
    assert!(
        r.is_ok(),
        "get_feature must succeed with stale cache after WS close, got: {:?}",
        r
    );

    // The connection status reflects that we are offline (WS was closed)
    let r = client.is_online();
    assert!(matches!(r, Ok(false)));

    // Clean-up: when `server` goes out of scope, it will destroy it's `_terminator` and
    // the server thread will be killed. When it goes away, the thread in the client
    // will enter a loop trying to reconnect to the server and failing because there is no
    // server listening at the URL... however, it will retry and retry because it's considered
    // a recoverable error (in prod, server might become alive again).
    // Finally, when the test session ends, the thread will be garbage-collected.
}

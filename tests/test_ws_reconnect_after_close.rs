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

// Integration test: verify the SDK reconnects to the WebSocket after the
// connection is closed and the server becomes available again.
//
// Flow:
//   1. Server accepts WS connection, sends heartbeat, serves config via HTTP.
//   2. SDK goes Online.
//   3. Server sends WS Close — SDK goes Offline but keeps stale config.
//   4. Server immediately starts listening again on the SAME port.
//   5. SDK's background thread retries and reconnects — SDK goes Online again.
//   6. Test asserts the SDK is Online and feature evaluation still works.

use ibm_appconfiguration_rust_sdk::{ConfigurationId, OfflineMode, ServiceAddress};

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{sleep, spawn};
use std::time::Duration;

mod common;

// How long to wait for the SDK to reconnect after the server restarts.
// The initial WS retry delay is ~15 s (±30% jitter), but the poll loop
// short-circuits as soon as it detects connectivity, so in a local test
// (loopback, no real internet gap) reconnection should happen well within
// the first backoff window.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(30);

fn load_json_payload() -> String {
    let mut mocked_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    mocked_data.push("data/data-dump-enterprise-plan-sdk-testing.json");
    std::fs::read_to_string(mocked_data).expect("test data file not found")
}

fn serve_http_config(listener: &TcpListener, json_payload: &str) {
    let (mut stream, _) = listener.accept().unwrap();
    {
        let buf_reader = BufReader::new(&stream);
        let _: Vec<_> = buf_reader
            .lines()
            .map(|l| l.unwrap())
            .take_while(|l| !l.is_empty())
            .collect();
    }
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        json_payload.len(),
        json_payload
    );
    stream.write_all(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

/// Runs a minimal server that:
///  • Accepts one WS connection and immediately sends a heartbeat.
///  • Accepts one HTTP config request and returns the fixture payload.
///  • Waits for `proceed_rx` before sending a WS Close frame.
///  • After the close, accepts a SECOND WS connection and heartbeat, then a
///    SECOND HTTP config request — simulating server restart on the same port.
///  • Signals `reconnected_tx` once the second connection is fully served.
fn run_server(listener: TcpListener, proceed_rx: Receiver<()>, reconnected_tx: Sender<()>) {
    let json_payload = load_json_payload();

    // ── First connection cycle ────────────────────────────────────────────────
    let (ws_stream, _) = listener.accept().unwrap();
    let mut ws = tungstenite::accept(ws_stream).unwrap();
    ws.send(tungstenite::Message::text("test message")).unwrap();

    serve_http_config(&listener, &json_payload);

    proceed_rx.recv().unwrap();
    let _ = ws.send(tungstenite::Message::Close(None));
    drop(ws);

    // ── Second connection cycle (reconnect) ───────────────────────────────────
    // The SDK will retry with backoff. Accept whenever it comes.
    let (ws_stream2, _) = listener.accept().unwrap();
    let mut ws2 = tungstenite::accept(ws_stream2).unwrap();
    ws2.send(tungstenite::Message::text("test message"))
        .unwrap();

    serve_http_config(&listener, &json_payload);

    // Signal the test that the reconnect is complete.
    let _ = reconnected_tx.send(());

    // Keep the WS alive until the test finishes (otherwise the SDK would
    // immediately see another disconnect).
    let _ = ws2.read(); // blocks until the SDK closes
}

#[test]
fn test_reconnects_after_ws_close() {
    // Bind once; the server thread will keep accepting connections on the same port.
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind failed");
    let port = listener.local_addr().unwrap().port();

    let (proceed_tx, proceed_rx) = channel::<()>();
    let (reconnected_tx, reconnected_rx) = channel::<()>();

    spawn(move || run_server(listener, proceed_rx, reconnected_tx));

    // ── Create SDK client ─────────────────────────────────────────────────────
    let address = ServiceAddress::new_without_ssl(
        "127.0.0.1".to_string(),
        Some(port),
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

    // ── Phase 1: go Online ────────────────────────────────────────────────────
    assert!(
        client.wait_until_online(),
        "SDK did not go Online initially"
    );
    assert!(matches!(client.is_online(), Ok(true)));
    let f = client.get_feature("f1");
    assert!(f.is_ok(), "get_feature failed while Online: {:?}", f);

    // ── Phase 2: server closes WS ─────────────────────────────────────────────
    proceed_tx.send(()).unwrap();
    // Give the SDK a moment to process the Close frame.
    sleep(Duration::from_millis(200));

    // Stale cache must still serve features even though we are Offline.
    let f = client.get_feature("f1");
    assert!(
        f.is_ok(),
        "get_feature must serve stale cache while Offline: {:?}",
        f
    );
    assert!(
        matches!(client.is_online(), Ok(false)),
        "SDK should be Offline after WS close"
    );

    // ── Phase 3: server restarts, SDK should reconnect ────────────────────────
    // Wait for the server to confirm it accepted the second connection.
    let reconnected = reconnected_rx.recv_timeout(RECONNECT_TIMEOUT);
    assert!(
        reconnected.is_ok(),
        "Server did not receive a reconnect within {:?}",
        RECONNECT_TIMEOUT
    );

    // Give the SDK thread a moment to process the new config and flip Online.
    let went_online = client.wait_until_online();
    assert!(
        went_online,
        "SDK did not return to Online after server restart"
    );
    assert!(matches!(client.is_online(), Ok(true)));

    // Feature evaluation must work again with fresh data.
    let f = client.get_feature("f1");
    assert!(f.is_ok(), "get_feature failed after reconnect: {:?}", f);
}

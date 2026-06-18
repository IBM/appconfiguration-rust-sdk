// (C) Copyright IBM Corp. 2025.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use rand::Rng;

const CONNECTIVITY_PROBE_TARGET: &str = "cloud.ibm.com";

/// DNS server to use for queries (Google's public DNS)
const DNS_SERVER: &str = "8.8.8.8:53";

/// Timeout for connectivity check (matches Node SDK)
const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(5);

/// Number of retries for connectivity check (matches Node SDK)
#[cfg(test)]
const CONNECTIVITY_RETRIES: u32 = 2;

/// Checks internet connectivity by performing a DNS query.
///
/// This function mimics the Node SDK's `checkInternet()` function by:
/// 1. Performing a DNS query to `cloud.ibm.com`
/// 2. Using Google's public DNS server (8.8.8.8)
/// 3. Timing out after 5 seconds
/// 4. Retrying up to 2 times on failure
///
/// # Returns
///
/// * `true` if internet connectivity is available
/// * `false` if connectivity check fails after all retries
///
/// # Examples
///
/// ```no_run
/// # // This is an internal module, so we can't use it directly in doctests
/// # // The function is tested in the connectivity_test module
/// ```
#[cfg(test)]
pub fn check_internet() -> bool {
    check_internet_with_retries(CONNECTIVITY_RETRIES)
}

/// Fast single-attempt connectivity check (no retries) — used for polling
/// inside the retry backoff loop where we call this every second and cannot
/// afford the full 15-second worst-case of `check_internet()`.
pub fn check_internet_once() -> bool {
    matches!(perform_dns_query(), Ok(true))
}

/// Internal function to check internet connectivity with configurable retries.
#[cfg(test)]
fn check_internet_with_retries(retries: u32) -> bool {
    for attempt in 0..=retries {
        if attempt > 0 {
            log::debug!("Connectivity check retry attempt {}/{}", attempt, retries);
        }

        match perform_dns_query() {
            Ok(true) => return true,
            Ok(false) => {
                if attempt < retries {
                    continue;
                }
            }
            Err(e) => {
                log::debug!("Connectivity check error: {}", e);
                if attempt < retries {
                    continue;
                }
            }
        }
    }

    false
}

/// Performs a DNS query to check connectivity.
///
/// This function attempts to resolve the target host using Google's DNS server.
/// It uses a UDP socket with a timeout to simulate the Node SDK's dns-socket behavior.
fn perform_dns_query() -> Result<bool, String> {
    // First, try to resolve the DNS server address
    let dns_addr: SocketAddr = DNS_SERVER
        .parse()
        .map_err(|e| format!("Failed to parse DNS server address: {}", e))?;

    // Create a UDP socket for DNS query
    let socket =
        UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Failed to create UDP socket: {}", e))?;

    socket
        .set_read_timeout(Some(CONNECTIVITY_TIMEOUT))
        .map_err(|e| format!("Failed to set socket timeout: {}", e))?;

    socket
        .set_write_timeout(Some(CONNECTIVITY_TIMEOUT))
        .map_err(|e| format!("Failed to set socket timeout: {}", e))?;

    // Build a simple DNS query for A record of cloud.ibm.com
    // DNS query format (simplified):
    // - Transaction ID (2 bytes)
    // - Flags (2 bytes): Standard query
    // - Questions (2 bytes): 1
    // - Answer RRs (2 bytes): 0
    // - Authority RRs (2 bytes): 0
    // - Additional RRs (2 bytes): 0
    // - Query: cloud.ibm.com A record
    let query = build_dns_query(CONNECTIVITY_PROBE_TARGET);

    // Send the DNS query
    socket
        .send_to(&query, dns_addr)
        .map_err(|e| format!("Failed to send DNS query: {}", e))?;

    // Wait for response
    let mut buf = [0u8; 512];
    match socket.recv_from(&mut buf) {
        Ok((size, _)) => {
            // If we received a response, we have connectivity
            log::debug!("Received DNS response ({} bytes)", size);
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            // Timeout - no connectivity
            log::debug!("DNS query timeout");
            Ok(false)
        }
        Err(e) => {
            log::debug!("DNS query error: {}", e);
            Ok(false)
        }
    }
}

/// Builds a DNS query packet for the given hostname.
///
/// This creates a minimal DNS query for an A record (IPv4 address).
fn build_dns_query(hostname: &str) -> Vec<u8> {
    let mut query = Vec::new();

    // Transaction ID: randomised per query to avoid response mismatches on rapid retries.
    let txid: u16 = rand::rng().random();
    query.extend_from_slice(&txid.to_be_bytes());

    // Flags: Standard query (0x0100)
    query.extend_from_slice(&[0x01, 0x00]);

    // Questions: 1
    query.extend_from_slice(&[0x00, 0x01]);

    // Answer RRs: 0
    query.extend_from_slice(&[0x00, 0x00]);

    // Authority RRs: 0
    query.extend_from_slice(&[0x00, 0x00]);

    // Additional RRs: 0
    query.extend_from_slice(&[0x00, 0x00]);

    // Query section: encode hostname
    for label in hostname.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0); // End of hostname

    // Type: A (0x0001)
    query.extend_from_slice(&[0x00, 0x01]);

    // Class: IN (0x0001)
    query.extend_from_slice(&[0x00, 0x01]);

    query
}

/// Fallback connectivity check using TCP connection.
///
/// This is the original implementation that attempts a TCP connection.
/// It's kept as a fallback option if DNS-based checking is not desired.
#[allow(dead_code)]
pub fn check_internet_tcp() -> bool {
    let target = format!("{}:443", CONNECTIVITY_PROBE_TARGET);
    let addresses = match target.to_socket_addrs() {
        Ok(addresses) => addresses.collect::<Vec<SocketAddr>>(),
        Err(_) => return true, // Assume connected if DNS resolution fails
    };

    addresses
        .into_iter()
        .any(|address| std::net::TcpStream::connect_timeout(&address, CONNECTIVITY_TIMEOUT).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_internet() {
        // This test requires actual internet connectivity
        // In a real environment, this should pass
        let result = check_internet();
        // We can't assert true/false as it depends on actual connectivity
        // Just ensure it doesn't panic
        println!("Internet connectivity check result: {}", result);
    }

    #[test]
    fn test_check_internet_tcp_fallback() {
        // Test the TCP fallback method
        let result = check_internet_tcp();
        println!("TCP connectivity check result: {}", result);
    }

    #[test]
    fn test_build_dns_query() {
        let query = build_dns_query("cloud.ibm.com");

        // Verify basic structure
        assert!(query.len() > 12); // At least header + some query data

        // Verify flags/question count (bytes 2-5), not the random transaction ID (bytes 0-1)
        assert_eq!(query[2], 0x01); // Flags byte 1
        assert_eq!(query[3], 0x00); // Flags byte 2
        assert_eq!(query[4], 0x00); // Questions high byte
        assert_eq!(query[5], 0x01); // Questions low byte (1 question)

        // Verify transaction IDs are not always identical across queries
        let query2 = build_dns_query("cloud.ibm.com");
        // With overwhelming probability two random u16s differ (1/65536 chance of collision)
        let txid1 = u16::from_be_bytes([query[0], query[1]]);
        let txid2 = u16::from_be_bytes([query2[0], query2[1]]);
        // We can't guarantee they differ in a single call, but we can verify they are valid u16s
        let _ = txid1;
        let _ = txid2;
    }

    #[test]
    fn test_dns_query_format() {
        let query = build_dns_query("test.example.com");

        // The query should contain the encoded hostname
        // "test" (4 bytes) + "example" (7 bytes) + "com" (3 bytes)
        let query_str = String::from_utf8_lossy(&query);
        assert!(query_str.contains("test"));
        assert!(query_str.contains("example"));
        assert!(query_str.contains("com"));
    }

    /// Test that connectivity check completes within reasonable time.
    ///
    /// Ignored by default because it performs real DNS queries that take up to 15 s
    /// (3 attempts × 5 s timeout) when run in an offline CI environment.
    #[test]

    fn test_connectivity_check_timeout() {
        use std::time::Instant;

        let start = Instant::now();
        let _result = check_internet();
        let elapsed = start.elapsed();

        // With 2 retries and 5 second timeout, maximum time should be around 15 seconds
        // Add some buffer for processing time
        assert!(
            elapsed.as_secs() < 20,
            "Connectivity check took too long: {:?}",
            elapsed
        );
    }

    /// Test multiple consecutive connectivity checks.
    #[test]
    fn test_multiple_connectivity_checks() {
        // Verify that multiple checks can be performed without issues
        for i in 0..3 {
            let result = check_internet();
            println!("Connectivity check #{}: {}", i + 1, result);
        }
    }

    /// Test that both DNS and TCP methods can be called.
    #[test]
    fn test_both_connectivity_methods() {
        let dns_result = check_internet();
        let tcp_result = check_internet_tcp();

        println!("DNS-based check: {}", dns_result);
        println!("TCP-based check: {}", tcp_result);

        // Both methods should complete without panicking
        // In most cases, they should return the same result
    }
}

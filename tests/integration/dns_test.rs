//! Integration test: DNS server startup and basic query resolution.
//!
//! This test starts the FreeIX DNS proxy on a random high port,
//! sends a DNS query, and verifies the response.

use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// Simple DNS query builder for testing.
/// Constructs a minimal DNS query packet for a given domain.
fn build_dns_query(domain: &str, query_id: u16) -> Vec<u8> {
    let mut packet = Vec::new();

    // Header
    packet.extend_from_slice(&query_id.to_be_bytes()); // ID
    packet.extend_from_slice(&[0x01, 0x00]); // Flags: standard query, recursion desired
    packet.extend_from_slice(&[0x00, 0x01]); // QDCOUNT: 1
    packet.extend_from_slice(&[0x00, 0x00]); // ANCOUNT: 0
    packet.extend_from_slice(&[0x00, 0x00]); // NSCOUNT: 0
    packet.extend_from_slice(&[0x00, 0x00]); // ARCOUNT: 0

    // Question section
    for label in domain.split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0x00); // Root label

    packet.extend_from_slice(&[0x00, 0x01]); // QTYPE: A
    packet.extend_from_slice(&[0x00, 0x01]); // QCLASS: IN

    packet
}

/// Parse the response code from a DNS response packet.
fn parse_rcode(response: &[u8]) -> u8 {
    if response.len() < 12 {
        panic!("DNS response too short: {} bytes", response.len());
    }
    response[3] & 0x0F
}

/// Parse the answer count from a DNS response packet.
fn parse_ancount(response: &[u8]) -> u16 {
    if response.len() < 12 {
        panic!("DNS response too short: {} bytes", response.len());
    }
    u16::from_be_bytes([response[6], response[7]])
}

/// Parse the query ID from a DNS response packet.
fn parse_query_id(response: &[u8]) -> u16 {
    if response.len() < 2 {
        panic!("DNS response too short: {} bytes", response.len());
    }
    u16::from_be_bytes([response[0], response[1]])
}

#[tokio::test]
async fn test_dns_server_starts_and_responds() {
    // Start the DNS server on a random port
    let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = freeix_dns::DnsServer::builder()
        .bind_addr(bind_addr)
        .upstream(vec!["8.8.8.8:53".parse().unwrap()])
        .build()
        .await
        .expect("Failed to build DNS server");

    let server_addr = server.local_addr();
    let _handle = tokio::spawn(async move {
        server.run().await.expect("DNS server failed");
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send a DNS query
    let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let query_id: u16 = 0xABCD;
    let query = build_dns_query("example.com", query_id);

    socket
        .send_to(&query, server_addr)
        .expect("Failed to send query");

    let mut buf = [0u8; 512];
    let (len, _) = socket
        .recv_from(&mut buf)
        .expect("Failed to receive response");

    let response = &buf[..len];

    // Verify response
    assert_eq!(parse_query_id(response), query_id, "Query ID mismatch");
    assert_eq!(parse_rcode(response), 0, "Expected NOERROR response code");
    assert!(
        parse_ancount(response) > 0,
        "Expected at least one answer record"
    );
}

#[tokio::test]
async fn test_dns_server_handles_invalid_query() {
    let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = freeix_dns::DnsServer::builder()
        .bind_addr(bind_addr)
        .upstream(vec!["8.8.8.8:53".parse().unwrap()])
        .build()
        .await
        .expect("Failed to build DNS server");

    let server_addr = server.local_addr();
    let _handle = tokio::spawn(async move {
        server.run().await.expect("DNS server failed");
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    // Send garbage data
    let garbage = vec![0xFF; 5];
    socket
        .send_to(&garbage, server_addr)
        .expect("Failed to send garbage");

    // Server should either respond with FORMERR or ignore the packet
    let mut buf = [0u8; 512];
    match socket.recv_from(&mut buf) {
        Ok((len, _)) => {
            let response = &buf[..len];
            let rcode = parse_rcode(response);
            assert_eq!(rcode, 1, "Expected FORMERR (1) for invalid query");
        }
        Err(_) => {
            // Timeout is acceptable - server may silently drop malformed packets
        }
    }
}

//! WinDivert-based packet filter for HTTP URL blocking and TLS SNI filtering.
//!
//! Uses WinDivert to intercept outbound TCP packets on ports 80/443.
//! - Port 80 (HTTP): inspects first data packet for Host header, blocks ad domains
//! - Port 443 (HTTPS): extracts SNI from TLS ClientHello, blocks ad domains
//!
//! Requires WinDivert.dll and WinDivert64.sys in the same directory as the exe.
//! Must run with Administrator privileges.

mod windivert_ffi;

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, info, warn};

pub use windivert_ffi::WinDivertHandle;

#[derive(Debug, Error)]
pub enum PacketFilterError {
    #[error("WinDivert failed to load: {0}")]
    LoadFailed(String),
    #[error("WinDivert open failed: {0}")]
    OpenFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Shared set of domains to block at packet level.
#[derive(Clone)]
pub struct PacketBlocklist {
    domains: Arc<RwLock<HashSet<String>>>,
}

impl PacketBlocklist {
    pub fn new() -> Self {
        Self {
            domains: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn add_domain(&self, domain: &str) {
        self.domains.write().insert(domain.to_lowercase());
    }

    pub fn add_domains(&self, domains: impl IntoIterator<Item = impl AsRef<str>>) {
        let mut set = self.domains.write();
        for d in domains {
            set.insert(d.as_ref().to_lowercase());
        }
    }

    pub fn should_block(&self, domain: &str) -> bool {
        let domain = domain.to_lowercase();
        let set = self.domains.read();

        // Check exact match and all parent domains
        let mut d = domain.as_str();
        loop {
            if set.contains(d) {
                return true;
            }
            match d.find('.') {
                Some(idx) => d = &d[idx + 1..],
                None => break,
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.domains.read().len()
    }
}

/// The main packet filter engine.
pub struct PacketFilter {
    blocklist: PacketBlocklist,
}

impl PacketFilter {
    pub fn new(blocklist: PacketBlocklist) -> Self {
        Self {
            blocklist,
        }
    }

    /// Start the packet filter in a background thread.
    /// Must be called from an admin process.
    pub fn start(&mut self) -> Result<(), PacketFilterError> {
        let blocklist = self.blocklist.clone();

        // Load WinDivert
        let wd = windivert_ffi::WinDivert::load()?;

        // Open handle for outbound TCP data on ports 80 and 443
        let filter = "outbound and tcp and (tcp.DstPort == 80 or tcp.DstPort == 443) and tcp.PayloadLength > 0";
        let handle = wd.open(filter, 0, 0)?;

        info!("WinDivert packet filter started");

        // Move wd into thread to keep DLL loaded
        std::thread::spawn(move || {
            let _wd = wd; // Keep DLL alive
            let mut buf = vec![0u8; 65535];
            let mut addr_buf = windivert_ffi::WinDivertAddress::default();

            loop {
                let len = match handle.recv(&mut buf, &mut addr_buf) {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("WinDivert recv error: {}", e);
                        break;
                    }
                };

                let packet = &buf[..len];

                // Parse IP + TCP headers to get payload offset
                let (dst_port, payload_offset) = match parse_tcp_packet(packet) {
                    Some(v) => v,
                    None => {
                        // Can't parse, just pass through
                        let _ = handle.send(packet, &addr_buf);
                        continue;
                    }
                };

                let payload = &packet[payload_offset..];

                let should_drop = if dst_port == 80 {
                    // HTTP: look for Host header
                    match extract_http_host(payload) {
                        Some(host) => {
                            if blocklist.should_block(&host) {
                                debug!(host = %host, "Blocked HTTP packet (WinDivert)");
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    }
                } else if dst_port == 443 {
                    // TLS: extract SNI from ClientHello
                    match extract_tls_sni(payload) {
                        Some(sni) => {
                            if blocklist.should_block(&sni) {
                                debug!(sni = %sni, "Blocked TLS ClientHello (WinDivert)");
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    }
                } else {
                    false
                };

                if !should_drop {
                    let _ = handle.send(packet, &addr_buf);
                }
                // If should_drop, we simply don't re-inject the packet (it's dropped)
            }
        });

        Ok(())
    }
}

/// Parse IPv4 TCP packet, return (dst_port, payload_offset).
fn parse_tcp_packet(packet: &[u8]) -> Option<(u16, usize)> {
    if packet.len() < 40 {
        return None;
    }

    // IPv4 check
    let version = (packet[0] >> 4) & 0x0F;
    if version != 4 {
        return None;
    }

    let ihl = (packet[0] & 0x0F) as usize * 4;
    if packet.len() < ihl + 20 {
        return None;
    }

    // Protocol check (TCP = 6)
    if packet[9] != 6 {
        return None;
    }

    let tcp_start = ihl;
    let dst_port = u16::from_be_bytes([packet[tcp_start + 2], packet[tcp_start + 3]]);
    let data_offset = ((packet[tcp_start + 12] >> 4) & 0x0F) as usize * 4;
    let payload_offset = tcp_start + data_offset;

    if payload_offset > packet.len() {
        return None;
    }

    Some((dst_port, payload_offset))
}

/// Extract Host header from HTTP request payload.
fn extract_http_host(payload: &[u8]) -> Option<String> {
    // Only look at the first ~2KB
    let text = std::str::from_utf8(&payload[..payload.len().min(2048)]).ok()?;

    // Must start with an HTTP method
    if !text.starts_with("GET ")
        && !text.starts_with("POST ")
        && !text.starts_with("HEAD ")
        && !text.starts_with("PUT ")
        && !text.starts_with("DELETE ")
        && !text.starts_with("CONNECT ")
        && !text.starts_with("OPTIONS ")
        && !text.starts_with("PATCH ")
    {
        return None;
    }

    for line in text.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some(host) = line.strip_prefix("Host: ").or_else(|| line.strip_prefix("host: ")) {
            return Some(host.split(':').next().unwrap_or(host).trim().to_lowercase());
        }
    }
    None
}

/// Extract SNI from TLS ClientHello.
fn extract_tls_sni(payload: &[u8]) -> Option<String> {
    // TLS record header: type(1) + version(2) + length(2)
    if payload.len() < 5 {
        return None;
    }
    if payload[0] != 0x16 {
        return None; // Not a handshake record
    }

    let record_len = u16::from_be_bytes([payload[3], payload[4]]) as usize;
    if payload.len() < 5 + record_len {
        return None;
    }

    let hs = &payload[5..];
    if hs.is_empty() || hs[0] != 0x01 {
        return None; // Not ClientHello
    }

    // ClientHello: type(1) + length(3) + version(2) + random(32) + session_id_len(1)...
    if hs.len() < 38 {
        return None;
    }

    let mut pos = 4; // skip type + length(3)
    pos += 2; // client version
    pos += 32; // random

    if pos >= hs.len() {
        return None;
    }

    // Session ID
    let session_id_len = hs[pos] as usize;
    pos += 1 + session_id_len;

    if pos + 2 > hs.len() {
        return None;
    }

    // Cipher suites
    let cs_len = u16::from_be_bytes([hs[pos], hs[pos + 1]]) as usize;
    pos += 2 + cs_len;

    if pos + 1 > hs.len() {
        return None;
    }

    // Compression methods
    let cm_len = hs[pos] as usize;
    pos += 1 + cm_len;

    if pos + 2 > hs.len() {
        return None;
    }

    // Extensions
    let ext_len = u16::from_be_bytes([hs[pos], hs[pos + 1]]) as usize;
    pos += 2;

    let ext_end = pos + ext_len;
    if ext_end > hs.len() {
        return None;
    }

    while pos + 4 <= ext_end {
        let ext_type = u16::from_be_bytes([hs[pos], hs[pos + 1]]);
        let ext_data_len = u16::from_be_bytes([hs[pos + 2], hs[pos + 3]]) as usize;
        pos += 4;

        if ext_type == 0x0000 {
            // SNI extension
            // SNI list: total_len(2) + type(1) + name_len(2) + name
            if ext_data_len >= 5 && pos + ext_data_len <= hs.len() {
                let name_len = u16::from_be_bytes([hs[pos + 3], hs[pos + 4]]) as usize;
                let name_start = pos + 5;
                if name_start + name_len <= hs.len() {
                    let sni = std::str::from_utf8(&hs[name_start..name_start + name_len]).ok()?;
                    return Some(sni.to_lowercase());
                }
            }
            return None;
        }

        pos += ext_data_len;
    }

    None
}

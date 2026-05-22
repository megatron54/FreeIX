use std::net::SocketAddr;
use std::time::Duration;

use hickory_proto::op::Message;
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::debug;

#[derive(Debug, Error)]
pub enum UpstreamError {
    #[error("all upstreams failed")]
    AllFailed,
    #[error("upstream timeout after {0:?}")]
    Timeout(Duration),
    #[error("resolve error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Proto(#[from] hickory_proto::error::ProtoError),
}

#[derive(Debug, Clone)]
pub enum UpstreamProtocol {
    Plain,
    DoH,
    DoT,
}

#[derive(Debug, Clone)]
pub enum UpstreamProvider {
    Cloudflare,
    AdGuard,
    Mullvad,
    Google,
    Custom(Vec<UpstreamEntry>),
}

#[derive(Debug, Clone)]
pub struct UpstreamEntry {
    pub address: SocketAddr,
    pub protocol: UpstreamProtocol,
    pub hostname: Option<String>,
    pub doh_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    pub providers: Vec<UpstreamProvider>,
    pub timeout: Duration,
    pub retries: u8,
}

impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            providers: vec![UpstreamProvider::AdGuard],
            timeout: Duration::from_secs(5),
            retries: 2,
        }
    }
}

impl UpstreamProvider {
    pub fn to_entries(&self) -> Vec<UpstreamEntry> {
        match self {
            Self::Cloudflare => vec![
                UpstreamEntry {
                    address: "1.1.1.1:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("cloudflare-dns.com".into()),
                    doh_url: None,
                },
                UpstreamEntry {
                    address: "1.0.0.1:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("cloudflare-dns.com".into()),
                    doh_url: None,
                },
            ],
            Self::AdGuard => vec![
                UpstreamEntry {
                    address: "94.140.14.14:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("dns.adguard-dns.com".into()),
                    doh_url: None,
                },
                UpstreamEntry {
                    address: "94.140.15.15:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("dns.adguard-dns.com".into()),
                    doh_url: None,
                },
            ],
            Self::Mullvad => vec![
                UpstreamEntry {
                    address: "194.242.2.2:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("dns.mullvad.net".into()),
                    doh_url: None,
                },
            ],
            Self::Google => vec![
                UpstreamEntry {
                    address: "8.8.8.8:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: None,
                    doh_url: None,
                },
                UpstreamEntry {
                    address: "8.8.4.4:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: None,
                    doh_url: None,
                },
            ],
            Self::Custom(entries) => entries.clone(),
        }
    }
}

pub struct UpstreamResolver {
    config: UpstreamConfig,
    /// Plain DNS server addresses to forward to
    servers: Vec<SocketAddr>,
}

impl UpstreamResolver {
    pub fn new(config: UpstreamConfig) -> Self {
        let mut servers = Vec::new();

        for provider in &config.providers {
            for entry in provider.to_entries() {
                match entry.protocol {
                    UpstreamProtocol::Plain | UpstreamProtocol::DoT => {
                        servers.push(entry.address);
                    }
                    UpstreamProtocol::DoH => {
                        // For now, use the IP directly on port 53 as fallback
                        let mut addr = entry.address;
                        addr.set_port(53);
                        servers.push(addr);
                    }
                }
            }
        }

        if servers.is_empty() {
            // Fallback to Google DNS
            servers.push("8.8.8.8:53".parse().unwrap());
            servers.push("8.8.4.4:53".parse().unwrap());
        }

        debug!(?servers, "upstream resolver initialized");

        Self { config, servers }
    }

    /// Resolve a DNS query by forwarding the raw packet to upstream servers.
    /// Tries each server in order until one responds.
    pub async fn resolve(&self, query: &Message) -> Result<Message, UpstreamError> {
        let query_bytes = query.to_bytes()?;

        for attempt in 0..=self.config.retries {
            for server in &self.servers {
                match self.forward_udp(&query_bytes, *server).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        if attempt == self.config.retries as u8 {
                            debug!(server = %server, error = %e, "upstream server failed");
                        }
                    }
                }
            }
        }

        Err(UpstreamError::AllFailed)
    }

    /// Forward a raw DNS query via UDP to the given server and return the response.
    async fn forward_udp(&self, query_bytes: &[u8], server: SocketAddr) -> Result<Message, UpstreamError> {
        // Bind to any available port (0 = OS picks)
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.send_to(query_bytes, server).await?;

        let mut buf = vec![0u8; 4096];
        let len = timeout(self.config.timeout, socket.recv(&mut buf))
            .await
            .map_err(|_| UpstreamError::Timeout(self.config.timeout))?
            .map_err(UpstreamError::Io)?;

        let response = Message::from_bytes(&buf[..len])?;
        Ok(response)
    }
}

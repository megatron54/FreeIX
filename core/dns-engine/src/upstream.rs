use std::net::SocketAddr;
use std::time::Duration;

use hickory_proto::op::Message;
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use thiserror::Error;
use tokio::time::timeout;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum UpstreamError {
    #[error("all upstreams failed")]
    AllFailed,
    #[error("upstream timeout after {0:?}")]
    Timeout(Duration),
    #[error("resolve error: {0}")]
    Resolve(#[from] hickory_resolver::error::ResolveError),
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
            providers: vec![UpstreamProvider::Cloudflare],
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
                    address: "1.1.1.1:443".parse().unwrap(),
                    protocol: UpstreamProtocol::DoH,
                    hostname: Some("cloudflare-dns.com".into()),
                    doh_url: Some("https://cloudflare-dns.com/dns-query".into()),
                },
                UpstreamEntry {
                    address: "1.0.0.1:443".parse().unwrap(),
                    protocol: UpstreamProtocol::DoH,
                    hostname: Some("cloudflare-dns.com".into()),
                    doh_url: Some("https://cloudflare-dns.com/dns-query".into()),
                },
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
                    address: "94.140.14.14:443".parse().unwrap(),
                    protocol: UpstreamProtocol::DoH,
                    hostname: Some("dns.adguard-dns.com".into()),
                    doh_url: Some("https://dns.adguard-dns.com/dns-query".into()),
                },
                UpstreamEntry {
                    address: "94.140.15.15:443".parse().unwrap(),
                    protocol: UpstreamProtocol::DoH,
                    hostname: Some("dns.adguard-dns.com".into()),
                    doh_url: Some("https://dns.adguard-dns.com/dns-query".into()),
                },
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
            Self::Mullvad => vec![UpstreamEntry {
                address: "194.242.2.2:53".parse().unwrap(),
                protocol: UpstreamProtocol::Plain,
                hostname: Some("dns.mullvad.net".into()),
                doh_url: None,
            }],
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

/// Manual DNS-over-HTTPS resolver using reqwest.
pub struct DohResolver {
    client: reqwest::Client,
    url: String,
}

impl DohResolver {
    pub fn new(url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build DoH client");
        Self {
            client,
            url: url.to_string(),
        }
    }

    pub async fn resolve(&self, query: &Message) -> Result<Message, UpstreamError> {
        let query_bytes = query.to_bytes().map_err(UpstreamError::Proto)?;

        let response = self
            .client
            .post(&self.url)
            .header("content-type", "application/dns-message")
            .header("accept", "application/dns-message")
            .body(query_bytes.to_vec())
            .send()
            .await
            .map_err(|e| {
                warn!(error = %e, url = %self.url, "DoH request failed");
                UpstreamError::AllFailed
            })?;

        if !response.status().is_success() {
            warn!(status = %response.status(), url = %self.url, "DoH non-success status");
            return Err(UpstreamError::AllFailed);
        }

        let bytes = response.bytes().await.map_err(|e| {
            warn!(error = %e, "DoH response body read failed");
            UpstreamError::AllFailed
        })?;
        let msg = Message::from_bytes(&bytes).map_err(UpstreamError::Proto)?;
        Ok(msg)
    }
}

pub struct UpstreamResolver {
    config: UpstreamConfig,
    resolvers: Vec<std::sync::Arc<TokioAsyncResolver>>,
    doh_resolvers: Vec<std::sync::Arc<DohResolver>>,
}

impl UpstreamResolver {
    pub fn new(config: UpstreamConfig) -> Self {
        let mut resolvers = Vec::new();
        let mut doh_resolvers = Vec::new();

        for provider in &config.providers {
            for entry in provider.to_entries() {
                match entry.protocol {
                    UpstreamProtocol::DoH => {
                        if let Some(url) = &entry.doh_url {
                            doh_resolvers.push(std::sync::Arc::new(DohResolver::new(url)));
                        }
                    }
                    UpstreamProtocol::DoT => {
                        // Use TCP as approximation for DoT with hickory
                        let ns = NameServerConfig::new(entry.address, Protocol::Tcp);
                        let mut resolver_config = ResolverConfig::new();
                        resolver_config.add_name_server(ns);

                        let mut opts = ResolverOpts::default();
                        opts.timeout = config.timeout;
                        opts.attempts = config.retries as usize;
                        opts.cache_size = 0;

                        let resolver = TokioAsyncResolver::tokio(resolver_config, opts);
                        resolvers.push(std::sync::Arc::new(resolver));
                    }
                    UpstreamProtocol::Plain => {
                        let ns = NameServerConfig::new(entry.address, Protocol::Udp);
                        let mut resolver_config = ResolverConfig::new();
                        resolver_config.add_name_server(ns);

                        let mut opts = ResolverOpts::default();
                        opts.timeout = config.timeout;
                        opts.attempts = config.retries as usize;
                        opts.cache_size = 0;

                        let resolver = TokioAsyncResolver::tokio(resolver_config, opts);
                        resolvers.push(std::sync::Arc::new(resolver));
                    }
                }
            }
        }

        Self {
            config,
            resolvers,
            doh_resolvers,
        }
    }

    /// Resolve a query by racing all upstream resolvers (plain + DoH) in parallel.
    /// Returns the first successful response.
    pub async fn resolve(&self, query: &Message) -> Result<Message, UpstreamError> {
        if self.resolvers.is_empty() && self.doh_resolvers.is_empty() {
            return Err(UpstreamError::AllFailed);
        }

        let queries = query.queries();
        if queries.is_empty() {
            return Err(UpstreamError::AllFailed);
        }

        let query_ref = &queries[0];
        let name = query_ref.name().clone();
        let record_type = query_ref.query_type();

        debug!(
            name = %name,
            record_type = ?record_type,
            "resolving via upstream"
        );

        // Build futures for standard resolvers
        let mut futs: Vec<tokio::task::JoinHandle<Result<Message, UpstreamError>>> = Vec::new();

        for resolver in &self.resolvers {
            let name = name.clone();
            let resolver = resolver.clone();
            let query_id = query.id();
            let query_queries = query.queries().to_vec();
            futs.push(tokio::spawn(async move {
                let lookup = resolver.lookup(name, record_type).await?;
                let mut response = Message::new();
                response.set_id(query_id);
                response.set_message_type(hickory_proto::op::MessageType::Response);
                response.set_op_code(hickory_proto::op::OpCode::Query);
                response.set_recursion_desired(true);
                response.set_recursion_available(true);
                response.add_queries(query_queries);
                for record in lookup.record_iter() {
                    response.add_answer(record.clone());
                }
                Ok(response)
            }));
        }

        // Build futures for DoH resolvers
        for doh in &self.doh_resolvers {
            let doh = doh.clone();
            let query_clone = query.clone();
            futs.push(tokio::spawn(async move {
                doh.resolve(&query_clone).await
            }));
        }

        // Race all futures
        let result = timeout(self.config.timeout, async {
            use tokio::sync::mpsc;
            let (tx, mut rx) = mpsc::channel(futs.len());

            for fut in futs {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = fut.await;
                    let _ = tx.send(result).await;
                });
            }
            drop(tx);

            while let Some(result) = rx.recv().await {
                match result {
                    Ok(Ok(msg)) => return Ok(msg),
                    _ => continue,
                }
            }
            Err(UpstreamError::AllFailed)
        })
        .await;

        match result {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(e)) => {
                warn!(error = %e, "all upstreams failed");
                Err(e)
            }
            Err(_) => {
                warn!("upstream resolution timed out");
                Err(UpstreamError::Timeout(self.config.timeout))
            }
        }
    }
}

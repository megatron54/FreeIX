use std::net::SocketAddr;
use std::time::Duration;

use hickory_proto::op::Message;
use hickory_proto::serialize::binary::BinDecodable;
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
                    address: "1.1.1.1:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("cloudflare-dns.com".into()),
                },
                UpstreamEntry {
                    address: "1.0.0.1:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("cloudflare-dns.com".into()),
                },
            ],
            Self::AdGuard => vec![
                UpstreamEntry {
                    address: "94.140.14.14:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("dns.adguard-dns.com".into()),
                },
                UpstreamEntry {
                    address: "94.140.15.15:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: Some("dns.adguard-dns.com".into()),
                },
            ],
            Self::Mullvad => vec![UpstreamEntry {
                address: "194.242.2.2:53".parse().unwrap(),
                protocol: UpstreamProtocol::Plain,
                hostname: Some("dns.mullvad.net".into()),
            }],
            Self::Google => vec![
                UpstreamEntry {
                    address: "8.8.8.8:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: None,
                },
                UpstreamEntry {
                    address: "8.8.4.4:53".parse().unwrap(),
                    protocol: UpstreamProtocol::Plain,
                    hostname: None,
                },
            ],
            Self::Custom(entries) => entries.clone(),
        }
    }
}

pub struct UpstreamResolver {
    config: UpstreamConfig,
    resolvers: Vec<std::sync::Arc<TokioAsyncResolver>>,
}

impl UpstreamResolver {
    pub fn new(config: UpstreamConfig) -> Self {
        let mut resolvers = Vec::new();

        for provider in &config.providers {
            for entry in provider.to_entries() {
                let protocol = match entry.protocol {
                    UpstreamProtocol::Plain => Protocol::Udp,
                    UpstreamProtocol::DoT => Protocol::Tcp,
                    UpstreamProtocol::DoH => Protocol::Tcp,
                };

                let ns = NameServerConfig::new(entry.address, protocol);
                let mut resolver_config = ResolverConfig::new();
                resolver_config.add_name_server(ns);

                let mut opts = ResolverOpts::default();
                opts.timeout = config.timeout;
                opts.attempts = config.retries as usize;
                opts.cache_size = 0; // We handle caching ourselves

                let resolver = TokioAsyncResolver::tokio(resolver_config, opts);
                resolvers.push(std::sync::Arc::new(resolver));
            }
        }

        Self { config, resolvers }
    }

    /// Resolve a query by racing all upstream resolvers in parallel.
    /// Returns the first successful response.
    pub async fn resolve(&self, query: &Message) -> Result<Message, UpstreamError> {
        if self.resolvers.is_empty() {
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

        // Race all resolvers in parallel
        let futs: Vec<_> = self
            .resolvers
            .iter()
            .map(|resolver| {
                let name = name.clone();
                let resolver = resolver.clone();
                async move { resolver.lookup(name, record_type).await }
            })
            .collect();

        let result = timeout(self.config.timeout, async {
            let (result, _) = futures_select_first(futs).await;
            result
        })
        .await;

        match result {
            Ok(Ok(lookup)) => {
                let mut response = Message::new();
                response.set_id(query.id());
                response.set_message_type(hickory_proto::op::MessageType::Response);
                response.set_op_code(hickory_proto::op::OpCode::Query);
                response.set_recursion_desired(true);
                response.set_recursion_available(true);
                response.add_queries(query.queries().to_vec());

                for record in lookup.record_iter() {
                    response.add_answer(record.clone());
                }

                Ok(response)
            }
            Ok(Err(e)) => {
                warn!(error = %e, "all upstreams failed");
                Err(UpstreamError::Resolve(e))
            }
            Err(_) => {
                warn!("upstream resolution timed out");
                Err(UpstreamError::Timeout(self.config.timeout))
            }
        }
    }
}

/// Simple future racing: returns the first Ok result, or the last Err.
async fn futures_select_first<F, T, E>(futs: Vec<F>) -> (Result<T, E>, usize)
where
    F: std::future::Future<Output = Result<T, E>> + Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
{
    use tokio::sync::mpsc;

    let (tx, mut rx) = mpsc::channel(futs.len());

    for (i, fut) in futs.into_iter().enumerate() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = fut.await;
            let _ = tx.send((result, i)).await;
        });
    }
    drop(tx);

    let mut last_err = None;
    while let Some((result, idx)) = rx.recv().await {
        match result {
            Ok(val) => return (Ok(val), idx),
            Err(e) => last_err = Some((e, idx)),
        }
    }

    let (e, idx) = last_err.expect("at least one future must complete");
    (Err(e), idx)
}

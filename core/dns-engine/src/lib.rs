pub mod cache;
pub mod upstream;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::Name;
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use parking_lot::RwLock;
use thiserror::Error;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use cache::{CacheKey, DnsCache};
use freeix_filtering_engine::{FilterEngine, FilterResult};
use upstream::{UpstreamConfig, UpstreamResolver};

#[derive(Debug, Error)]
pub enum DnsServerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("server already running")]
    AlreadyRunning,
    #[error("server not running")]
    NotRunning,
    #[error("failed to bind to {addr}: {source}")]
    Bind { addr: SocketAddr, source: std::io::Error },
    #[error("upstream error: {0}")]
    Upstream(#[from] upstream::UpstreamError),
    #[error("proto error: {0}")]
    Proto(#[from] hickory_proto::error::ProtoError),
}

#[derive(Debug, Clone)]
pub struct DnsServerConfig {
    pub listen_addr: SocketAddr,
    pub upstream: UpstreamConfig,
    pub cache_size: usize,
    pub max_ttl: Duration,
    pub min_ttl: Duration,
}

impl Default for DnsServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:53".parse().unwrap(),
            upstream: UpstreamConfig::default(),
            cache_size: 10_000,
            max_ttl: Duration::from_secs(86400),
            min_ttl: Duration::from_secs(60),
        }
    }
}

pub struct DnsServer {
    config: DnsServerConfig,
    filter_engine: Arc<FilterEngine>,
    cache: Arc<DnsCache>,
    upstream: Arc<UpstreamResolver>,
    shutdown_tx: RwLock<Option<watch::Sender<bool>>>,
}

impl DnsServer {
    pub fn new(config: DnsServerConfig, filter_engine: Arc<FilterEngine>) -> Self {
        let cache = Arc::new(DnsCache::new(config.cache_size));
        let upstream = Arc::new(UpstreamResolver::new(config.upstream.clone()));

        Self {
            config,
            filter_engine,
            cache,
            upstream,
            shutdown_tx: RwLock::new(None),
        }
    }

    pub async fn start(&self) -> Result<(), DnsServerError> {
        if self.shutdown_tx.read().is_some() {
            return Err(DnsServerError::AlreadyRunning);
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let addr = self.config.listen_addr;
        let udp_socket = UdpSocket::bind(addr)
            .await
            .map_err(|e| DnsServerError::Bind { addr, source: e })?;
        let tcp_listener = TcpListener::bind(addr)
            .await
            .map_err(|e| DnsServerError::Bind { addr, source: e })?;

        info!(%addr, "DNS server listening");

        // Spawn UDP handler
        {
            let cache = self.cache.clone();
            let upstream = self.upstream.clone();
            let filter = self.filter_engine.clone();
            let config = self.config.clone();
            let mut rx = shutdown_rx.clone();
            let socket = Arc::new(udp_socket);

            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    tokio::select! {
                        _ = rx.changed() => break,
                        result = socket.recv_from(&mut buf) => {
                            match result {
                                Ok((len, src)) => {
                                    let data = Bytes::copy_from_slice(&buf[..len]);
                                    let socket = socket.clone();
                                    let cache = cache.clone();
                                    let upstream = upstream.clone();
                                    let filter = filter.clone();
                                    let config = config.clone();

                                    tokio::spawn(async move {
                                        if let Ok(response) = handle_query(&data, &filter, &cache, &upstream, &config).await {
                                            if let Ok(bytes) = response.to_bytes() {
                                                let _ = socket.send_to(&bytes, src).await;
                                            }
                                        }
                                    });
                                }
                                Err(e) => {
                                    warn!(error = %e, "UDP recv error");
                                }
                            }
                        }
                    }
                }
                info!("UDP handler shut down");
            });
        }

        // Spawn TCP handler
        {
            let cache = self.cache.clone();
            let upstream = self.upstream.clone();
            let filter = self.filter_engine.clone();
            let config = self.config.clone();
            let mut rx = shutdown_rx;

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = rx.changed() => break,
                        result = tcp_listener.accept() => {
                            match result {
                                Ok((stream, _src)) => {
                                    let cache = cache.clone();
                                    let upstream = upstream.clone();
                                    let filter = filter.clone();
                                    let config = config.clone();

                                    tokio::spawn(async move {
                                        if let Err(e) = handle_tcp_connection(stream, &filter, &cache, &upstream, &config).await {
                                            debug!(error = %e, "TCP connection error");
                                        }
                                    });
                                }
                                Err(e) => {
                                    warn!(error = %e, "TCP accept error");
                                }
                            }
                        }
                    }
                }
                info!("TCP handler shut down");
            });
        }

        *self.shutdown_tx.write() = Some(shutdown_tx);
        Ok(())
    }

    pub fn stop(&self) -> Result<(), DnsServerError> {
        let mut guard = self.shutdown_tx.write();
        if let Some(tx) = guard.take() {
            let _ = tx.send(true);
            info!("DNS server stopping");
            Ok(())
        } else {
            Err(DnsServerError::NotRunning)
        }
    }

    pub fn is_running(&self) -> bool {
        self.shutdown_tx.read().is_some()
    }

    pub fn cache_stats(&self) -> cache::CacheStats {
        self.cache.stats()
    }
}

async fn handle_tcp_connection(
    mut stream: tokio::net::TcpStream,
    filter: &FilterEngine,
    cache: &DnsCache,
    upstream: &UpstreamResolver,
    config: &DnsServerConfig,
) -> Result<(), DnsServerError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // DNS over TCP: 2-byte length prefix
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    let response = handle_query(&buf, filter, cache, upstream, config).await?;
    let bytes = response.to_bytes()?;

    let len_bytes = (bytes.len() as u16).to_be_bytes();
    stream.write_all(&len_bytes).await?;
    stream.write_all(&bytes).await?;

    Ok(())
}

async fn handle_query(
    data: &[u8],
    filter: &FilterEngine,
    cache: &DnsCache,
    upstream: &UpstreamResolver,
    config: &DnsServerConfig,
) -> Result<Message, DnsServerError> {
    let request = Message::from_bytes(data)?;
    let id = request.id();

    let queries = request.queries();
    if queries.is_empty() {
        return Ok(make_servfail(&request));
    }

    let query = &queries[0];
    let domain = query.name().to_string().trim_end_matches('.').to_lowercase();
    let record_type = query.query_type();

    debug!(%domain, ?record_type, "incoming query");

    // Check filter engine
    match filter.is_blocked(&domain) {
        FilterResult::Blocked { rule } => {
            debug!(%domain, %rule, "blocked by filter");
            return Ok(make_nxdomain(&request));
        }
        FilterResult::Allowed { .. } | FilterResult::NotMatched => {}
    }

    // Check cache
    let cache_key = CacheKey {
        name: domain.clone(),
        record_type: record_type.into(),
    };

    if let Some(mut cached) = cache.get(&cache_key) {
        cached.set_id(id);
        return Ok(cached);
    }

    // Forward to upstream
    match upstream.resolve(&request).await {
        Ok(mut response) => {
            response.set_id(id);

            // Determine TTL from response records
            let ttl = response
                .answers()
                .iter()
                .map(|r| Duration::from_secs(r.ttl() as u64))
                .min()
                .unwrap_or(config.min_ttl)
                .clamp(config.min_ttl, config.max_ttl);

            cache.insert(cache_key, response.clone(), ttl);
            Ok(response)
        }
        Err(e) => {
            warn!(%domain, error = %e, "upstream resolution failed");
            Ok(make_servfail(&request))
        }
    }
}

fn make_nxdomain(request: &Message) -> Message {
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_response_code(ResponseCode::NXDomain);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.add_queries(request.queries().to_vec());
    response
}

fn make_servfail(request: &Message) -> Message {
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_response_code(ResponseCode::ServFail);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.add_queries(request.queries().to_vec());
    response
}

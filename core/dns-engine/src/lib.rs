pub mod cache;
pub mod upstream;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::serialize::binary::BinEncodable;
use hickory_proto::serialize::binary::BinDecodable;
use parking_lot::RwLock;
use serde::Serialize;
use thiserror::Error;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::watch;
use tracing::{info, trace, warn};

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

/// Information about a processed DNS query.
#[derive(Debug, Clone, Serialize)]
pub struct QueryInfo {
    pub domain: String,
    pub query_type: String,
    pub blocked: bool,
    pub cached: bool,
    pub rule: Option<String>,
    pub response_time_ms: u32,
}

/// Callback invoked for each DNS query processed.
pub type QueryCallback = Arc<dyn Fn(QueryInfo) + Send + Sync>;

pub struct DnsServer {
    config: DnsServerConfig,
    filter_engine: Arc<FilterEngine>,
    cache: Arc<DnsCache>,
    upstream: Arc<UpstreamResolver>,
    shutdown_tx: RwLock<Option<watch::Sender<bool>>>,
    query_callback: Option<QueryCallback>,
    pub total_queries: AtomicU64,
    pub blocked_queries: AtomicU64,
    pub cache_hits: AtomicU64,
}

impl DnsServer {
    pub fn new(
        config: DnsServerConfig,
        filter_engine: Arc<FilterEngine>,
        callback: Option<QueryCallback>,
    ) -> Self {
        let cache = Arc::new(DnsCache::new(config.cache_size));
        let upstream = Arc::new(UpstreamResolver::new(config.upstream.clone()));

        Self {
            config,
            filter_engine,
            cache,
            upstream,
            shutdown_tx: RwLock::new(None),
            query_callback: callback,
            total_queries: AtomicU64::new(0),
            blocked_queries: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
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
            let callback = self.query_callback.clone();
            let total_queries = &self.total_queries as *const AtomicU64 as usize;
            let blocked_queries = &self.blocked_queries as *const AtomicU64 as usize;
            let cache_hits = &self.cache_hits as *const AtomicU64 as usize;
            let mut rx = shutdown_rx.clone();
            let socket = Arc::new(udp_socket);

            // We need to use Arc-based counters for the spawned tasks
            // Since self won't live long enough, we pass the callback only
            let cb = callback.clone();

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
                                    let cb = cb.clone();

                                    tokio::spawn(async move {
                                        match handle_query(&data, &filter, &cache, &upstream, &config, cb.as_ref()).await {
                                            Ok(response) => {
                                                match response.to_bytes() {
                                                    Ok(bytes) => {
                                                        let _ = socket.send_to(&bytes, src).await;
                                                    }
                                                    Err(e) => {
                                                        tracing::error!(error = %e, "failed to serialize DNS response");
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(error = %e, "handle_query failed");
                                            }
                                        }
                                    });
                                }
                                Err(e) => {
                                    // Error 10054 on Windows is an ICMP unreachable from a previous send
                                    // (common with ICS/WSL). It's not a real error — just retry.
                                    let is_windows_10054 = e.raw_os_error() == Some(10054);
                                    if !is_windows_10054 {
                                        warn!(error = %e, "UDP recv error");
                                    }
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
            let callback = self.query_callback.clone();
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
                                    let cb = callback.clone();

                                    tokio::spawn(async move {
                                        if let Err(e) = handle_tcp_connection(stream, &filter, &cache, &upstream, &config, cb.as_ref()).await {
                                            trace!(error = %e, "TCP connection error");
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
    callback: Option<&QueryCallback>,
) -> Result<(), DnsServerError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // DNS over TCP: 2-byte length prefix
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    let response = handle_query(&buf, filter, cache, upstream, config, callback).await?;
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
    callback: Option<&QueryCallback>,
) -> Result<Message, DnsServerError> {
    let start = Instant::now();
    let request = Message::from_bytes(data)?;
    let id = request.id();

    let queries = request.queries();
    if queries.is_empty() {
        return Ok(make_servfail(&request));
    }

    let query = &queries[0];
    let domain = query.name().to_string().trim_end_matches('.').to_lowercase();
    let record_type = query.query_type();

    trace!(%domain, ?record_type, "incoming query");

    // NOTE: Windows DNS client appends search suffixes (e.g., .home, .local, .lan) to queries
    // before sending them. For example, a lookup for "google.com" may first be sent as
    // "google.com.home". These suffixed queries will pass through the filter (not blocked) and
    // get forwarded upstream where they receive NXDOMAIN. This is expected behavior — the cache
    // will store the NXDOMAIN response so subsequent identical queries are fast, and the Windows
    // DNS client will automatically retry without the suffix.

    // Check filter engine
    match filter.is_blocked(&domain) {
        FilterResult::Blocked { rule } => {
            info!(%domain, %rule, "blocked by filter");
            let elapsed = start.elapsed().as_millis() as u32;
            if let Some(cb) = callback {
                cb(QueryInfo {
                    domain: domain.clone(),
                    query_type: format!("{:?}", record_type),
                    blocked: true,
                    cached: false,
                    rule: Some(rule),
                    response_time_ms: elapsed,
                });
            }
            return Ok(make_blocked_response(&request));
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
        let elapsed = start.elapsed().as_millis() as u32;
        if let Some(cb) = callback {
            cb(QueryInfo {
                domain: domain.clone(),
                query_type: format!("{:?}", record_type),
                blocked: false,
                cached: true,
                rule: None,
                response_time_ms: elapsed,
            });
        }
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

            let elapsed = start.elapsed().as_millis() as u32;
            if let Some(cb) = callback {
                cb(QueryInfo {
                    domain: domain.clone(),
                    query_type: format!("{:?}", record_type),
                    blocked: false,
                    cached: false,
                    rule: None,
                    response_time_ms: elapsed,
                });
            }

            Ok(response)
        }
        Err(e) => {
            warn!(%domain, error = %e, "upstream resolution failed");
            Ok(make_servfail(&request))
        }
    }
}

// NOTE on in-flight deduplication: Windows DNS client typically sends A + AAAA +
// search-suffix variants simultaneously. These are technically different queries
// (different record types or names), so they are not true duplicates. The cache with
// min_ttl=60s already deduplicates repeated identical queries after the first resolution.
// True in-flight coalescing (merging concurrent identical queries before the first
// resolves) would require a shared broadcast map keyed by (domain, qtype). This is not
// implemented yet because the performance impact is negligible for a local resolver.

/// Returns a well-formed response for blocked domains.
/// - A queries get 0.0.0.0
/// - AAAA queries get ::
/// - Other types get an empty NOERROR (no answers)
///
/// This avoids NXDOMAIN which can trigger retry/search-suffix behavior in clients.
fn make_blocked_response(request: &Message) -> Message {
    use hickory_proto::rr::{DNSClass, RData, Record, RecordType};

    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_response_code(ResponseCode::NoError);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.add_queries(request.queries().to_vec());

    if let Some(query) = request.queries().first() {
        let name = query.name().clone();
        match query.query_type() {
            RecordType::A => {
                let mut record = Record::from_rdata(
                    name,
                    300,
                    RData::A(std::net::Ipv4Addr::UNSPECIFIED.into()),
                );
                record.set_dns_class(DNSClass::IN);
                response.add_answer(record);
            }
            RecordType::AAAA => {
                let mut record = Record::from_rdata(
                    name,
                    300,
                    RData::AAAA(std::net::Ipv6Addr::UNSPECIFIED.into()),
                );
                record.set_dns_class(DNSClass::IN);
                response.add_answer(record);
            }
            _ => {
                // For other record types, return empty NOERROR
            }
        }
    }

    response
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

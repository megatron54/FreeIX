//! Transparent HTTPS MITM proxy.
//!
//! Accepts connections as an explicit HTTP proxy (browsers send CONNECT for HTTPS).
//! For HTTPS: intercepts via TLS MITM, inspects URLs, blocks ad requests.
//! For HTTP: inspects URL, blocks or forwards.

use std::sync::Arc;

use freeix_ca::RootCa;
use lru::LruCache;
use parking_lot::Mutex;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::ServerConfig;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tracing::{debug, info, trace, warn};

use crate::filter::UrlFilter;
use crate::ProxyError;

pub async fn run_proxy(
    port: u16,
    ca: Arc<RootCa>,
    filter: Arc<UrlFilter>,
) -> Result<(), ProxyError> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    info!(port, "HTTP/HTTPS proxy listening");

    let cert_cache: Arc<Mutex<LruCache<String, Arc<ServerConfig>>>> =
        Arc::new(Mutex::new(LruCache::new(std::num::NonZeroUsize::new(1000).unwrap())));

    loop {
        let (stream, _) = listener.accept().await?;
        let ca = ca.clone();
        let filter = filter.clone();
        let cert_cache = cert_cache.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, ca, filter, cert_cache).await {
                trace!(error = %e, "client error");
            }
        });
    }
}

async fn handle_client(
    stream: TcpStream,
    ca: Arc<RootCa>,
    filter: Arc<UrlFilter>,
    cert_cache: Arc<Mutex<LruCache<String, Arc<ServerConfig>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buf_reader = BufReader::new(stream);

    let mut request_line = String::new();
    buf_reader.read_line(&mut request_line).await?;

    if request_line.is_empty() {
        return Ok(());
    }

    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let target = parts[1];

    if method.eq_ignore_ascii_case("CONNECT") {
        handle_connect(buf_reader, target, ca, filter, cert_cache).await
    } else {
        handle_http(buf_reader, &request_line, filter).await
    }
}

/// Handle HTTP CONNECT — HTTPS tunneling with MITM.
async fn handle_connect(
    mut buf_reader: BufReader<TcpStream>,
    target: &str,
    ca: Arc<RootCa>,
    filter: Arc<UrlFilter>,
    cert_cache: Arc<Mutex<LruCache<String, Arc<ServerConfig>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Consume remaining headers
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
    }

    let host = target.split(':').next().unwrap_or(target).to_string();
    let port: u16 = target.split(':').nth(1).and_then(|p| p.parse().ok()).unwrap_or(443);

    // Send 200 to client
    let mut client_stream = buf_reader.into_inner();
    client_stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;

    // Decide: do we want to MITM this connection?
    // Only MITM domains where we have ad-blocking rules to apply.
    // For other domains, just tunnel directly (faster, no cert issues).
    let should_mitm = should_intercept(&host, &filter);

    if !should_mitm {
        // Plain tunnel — just relay bytes bidirectionally
        let mut upstream = TcpStream::connect(format!("{}:{}", host, port)).await?;
        let (mut client_read, mut client_write) = tokio::io::split(client_stream);
        let (mut upstream_read, mut upstream_write) = tokio::io::split(upstream);

        tokio::select! {
            r = tokio::io::copy(&mut client_read, &mut upstream_write) => { let _ = r; }
            r = tokio::io::copy(&mut upstream_read, &mut client_write) => { let _ = r; }
        }
        return Ok(());
    }

    // MITM: accept TLS from client with our forged cert
    let tls_config = get_or_create_tls_config(&host, &ca, &cert_cache)?;
    let acceptor = TlsAcceptor::from(tls_config);

    let tls_stream = match acceptor.accept(client_stream).await {
        Ok(s) => s,
        Err(e) => {
            trace!(host = %host, error = %e, "TLS accept failed");
            return Ok(());
        }
    };

    // Connect to real server
    let upstream_stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let client_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );
    let connector = tokio_rustls::TlsConnector::from(client_config);
    let server_name = ServerName::try_from(host.clone())?;
    let upstream_tls = match connector.connect(server_name, upstream_stream).await {
        Ok(s) => s,
        Err(e) => {
            trace!(host = %host, error = %e, "upstream TLS failed");
            return Ok(());
        }
    };

    // Now we have decrypted streams on both sides.
    // Read HTTP requests from client, check URL filter, forward or block.
    let mut client_buf = BufReader::new(tls_stream);
    let mut upstream_tls = upstream_tls;

    // Process one request (simplified — no keep-alive for MITM connections)
    let mut req_line = String::new();
    match client_buf.read_line(&mut req_line).await {
        Ok(0) | Err(_) => return Ok(()),
        _ => {}
    }

    let parts: Vec<&str> = req_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let path = parts[1];
    let url = format!("https://{}{}", host, path);

    // Read all headers
    let mut headers_raw = req_line.clone();
    loop {
        let mut line = String::new();
        client_buf.read_line(&mut line).await?;
        headers_raw.push_str(&line);
        if line.trim().is_empty() {
            break;
        }
    }

    // Check filter
    if filter.should_block(&host, path, &url) {
        info!(%url, "HTTPS request blocked by URL filter");
        let resp = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        client_buf.get_mut().write_all(resp).await?;
        return Ok(());
    }

    // Forward request to upstream
    upstream_tls.write_all(headers_raw.as_bytes()).await?;
    upstream_tls.flush().await?;

    // Relay response back to client
    let mut buf = vec![0u8; 32768];
    loop {
        match upstream_tls.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if client_buf.get_mut().write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Handle plain HTTP proxy request.
async fn handle_http(
    mut buf_reader: BufReader<TcpStream>,
    request_line: &str,
    filter: Arc<UrlFilter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 3 {
        return Ok(());
    }

    let full_url = parts[1];
    let http_version = parts[2];

    // Extract host and path from full URL
    let after_scheme = full_url.strip_prefix("http://").unwrap_or(full_url);
    let (host_port, path) = match after_scheme.find('/') {
        Some(i) => (&after_scheme[..i], &after_scheme[i..]),
        None => (after_scheme, "/"),
    };
    let host = host_port.split(':').next().unwrap_or(host_port);

    // Read all headers
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        headers.push(line);
    }

    // Check URL filter
    if filter.should_block(host, path, full_url) {
        info!(%full_url, "HTTP request blocked by URL filter");
        let resp = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        buf_reader.get_mut().write_all(resp).await?;
        return Ok(());
    }

    // Connect to upstream
    let upstream_addr = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{}:80", host_port)
    };

    let mut upstream = match TcpStream::connect(&upstream_addr).await {
        Ok(s) => s,
        Err(_) => {
            let resp = b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n";
            buf_reader.get_mut().write_all(resp).await?;
            return Ok(());
        }
    };

    // Send request with relative path to upstream
    let req_line = format!("{} {} {}\r\n", parts[0], path, http_version);
    upstream.write_all(req_line.as_bytes()).await?;
    for h in &headers {
        upstream.write_all(h.as_bytes()).await?;
    }
    upstream.write_all(b"\r\n").await?;
    upstream.flush().await?;

    // Bidirectional relay for response
    let mut client_stream = buf_reader.into_inner();
    let (mut cr, mut cw) = tokio::io::split(client_stream);
    let (mut ur, mut uw) = tokio::io::split(upstream);

    // Just copy upstream response to client
    tokio::io::copy(&mut ur, &mut cw).await?;

    Ok(())
}

/// Determine if we should MITM a particular host.
/// Only intercept domains where URL-level filtering is useful.
fn should_intercept(_host: &str, _filter: &UrlFilter) -> bool {
    // MITM disabled for now — Chrome/Firefox reject forged certs for pinned domains
    // (Google, Facebook, etc.) even with our CA in the system store.
    // DNS-level blocking handles domain-based ads. For YouTube video ads,
    // a browser extension approach is needed.
    //
    // TODO: Re-enable for non-pinned ad domains only (adnxs.com, taboola.com, etc.)
    // once we have a reliable list of domains that don't use HSTS/CT pinning.
    return false;

    #[allow(unreachable_code)]
    let intercept_domains: [&str; 0] = [];

    for domain in &intercept_domains {
        if _host == *domain || _host.ends_with(&format!(".{}", domain)) {
            return true;
        }
    }
    false
}

fn get_or_create_tls_config(
    domain: &str,
    ca: &RootCa,
    cache: &Arc<Mutex<LruCache<String, Arc<ServerConfig>>>>,
) -> Result<Arc<ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
    {
        let mut cache_lock = cache.lock();
        if let Some(config) = cache_lock.get(domain) {
            return Ok(config.clone());
        }
    }

    let (cert_der, key_der) = ca.issue_cert(domain)?;
    let cert = CertificateDer::from(cert_der);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_der));

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)?;

    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let config = Arc::new(config);

    {
        let mut cache_lock = cache.lock();
        cache_lock.put(domain.to_string(), config.clone());
    }

    Ok(config)
}

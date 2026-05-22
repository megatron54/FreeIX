//! Transparent HTTPS MITM proxy.
//!
//! Accepts redirected TCP connections (via system proxy settings),
//! handles CONNECT tunnels for HTTPS, inspects HTTP requests,
//! and blocks ad URLs.

use std::sync::Arc;

use bytes::Bytes;
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

/// Run the HTTP proxy on the given port.
/// Browsers/system connect to this as an explicit HTTP proxy.
/// HTTPS traffic uses the CONNECT method for tunneling.
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
                trace!(error = %e, "client connection error");
            }
        });
    }
}

/// Handle a client connection to the proxy.
/// Reads the first HTTP request line to determine if it's CONNECT (HTTPS) or a regular HTTP request.
async fn handle_client(
    stream: TcpStream,
    ca: Arc<RootCa>,
    filter: Arc<UrlFilter>,
    cert_cache: Arc<Mutex<LruCache<String, Arc<ServerConfig>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buf_reader = BufReader::new(stream);

    // Read the first line (request line)
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
        // HTTPS tunneling via CONNECT
        handle_connect(buf_reader, target, ca, filter, cert_cache).await
    } else {
        // Plain HTTP request (method + full URL)
        handle_plain_http(buf_reader, &request_line, filter).await
    }
}

/// Handle CONNECT method — establish TLS MITM tunnel.
async fn handle_connect(
    mut buf_reader: BufReader<TcpStream>,
    target: &str,
    ca: Arc<RootCa>,
    filter: Arc<UrlFilter>,
    cert_cache: Arc<Mutex<LruCache<String, Arc<ServerConfig>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Read remaining headers (until empty line)
    let mut header_line = String::new();
    loop {
        header_line.clear();
        buf_reader.read_line(&mut header_line).await?;
        if header_line.trim().is_empty() {
            break;
        }
    }

    // Extract host from target (host:port)
    let host = target.split(':').next().unwrap_or(target).to_string();

    // Send 200 Connection Established
    let mut stream = buf_reader.into_inner();
    stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;

    // Now we have a raw TCP connection to the client.
    // Perform TLS MITM: accept TLS from client with our forged cert,
    // then connect to the real server, relay filtered data.

    let tls_config = get_or_create_tls_config(&host, &ca, &cert_cache)?;
    let acceptor = TlsAcceptor::from(tls_config);

    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(e) => {
            trace!(host = %host, error = %e, "TLS accept failed (client may have rejected cert)");
            return Ok(());
        }
    };

    // Read the decrypted HTTP request from the client
    let mut client = BufReader::new(tls_stream);

    // Process potentially multiple HTTP requests (keep-alive)
    loop {
        let mut request_line = String::new();
        match client.read_line(&mut request_line).await {
            Ok(0) => break, // Connection closed
            Ok(_) => {}
            Err(_) => break,
        }

        if request_line.trim().is_empty() {
            break;
        }

        let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
        if parts.len() < 2 {
            break;
        }

        let path = parts[1];
        let url = format!("https://{}{}", host, path);

        // Read headers
        let mut headers = Vec::new();
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            client.read_line(&mut line).await?;
            if line.trim().is_empty() {
                break;
            }
            if line.to_lowercase().starts_with("content-length:") {
                content_length = line.split(':').nth(1)
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
            }
            headers.push(line);
        }

        // Check URL filter
        if filter.should_block(&host, path, &url) {
            info!(%url, "Blocked by URL filter");
            // Send a minimal blocked response to client
            let response = b"HTTP/1.1 204 No Content\r\nConnection: keep-alive\r\nContent-Length: 0\r\n\r\n";
            client.get_mut().write_all(response).await?;

            // Consume request body if any
            if content_length > 0 {
                let mut discard = vec![0u8; content_length];
                let _ = client.read_exact(&mut discard).await;
            }
            continue;
        }

        // Forward to real server
        let upstream_addr = format!("{}:443", host);
        let upstream_stream = match TcpStream::connect(&upstream_addr).await {
            Ok(s) => s,
            Err(e) => {
                trace!(host = %host, error = %e, "upstream connect failed");
                let response = b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n";
                client.get_mut().write_all(response).await?;
                break;
            }
        };

        // TLS to upstream
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let client_config = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth(),
        );
        let connector = tokio_rustls::TlsConnector::from(client_config);
        let server_name = match ServerName::try_from(host.clone()) {
            Ok(sn) => sn,
            Err(_) => break,
        };
        let mut upstream_tls = match connector.connect(server_name, upstream_stream).await {
            Ok(s) => s,
            Err(e) => {
                trace!(host = %host, error = %e, "upstream TLS failed");
                let response = b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n";
                client.get_mut().write_all(response).await?;
                break;
            }
        };

        // Send the request to upstream
        upstream_tls.write_all(request_line.as_bytes()).await?;
        for h in &headers {
            upstream_tls.write_all(h.as_bytes()).await?;
        }
        upstream_tls.write_all(b"\r\n").await?;

        // Send body if present
        if content_length > 0 {
            let mut body = vec![0u8; content_length];
            client.read_exact(&mut body).await?;
            upstream_tls.write_all(&body).await?;
        }
        upstream_tls.flush().await?;

        // Read response from upstream and forward to client
        let mut response_buf = vec![0u8; 65536];
        loop {
            match upstream_tls.read(&mut response_buf).await {
                Ok(0) => break,
                Ok(n) => {
                    client.get_mut().write_all(&response_buf[..n]).await?;
                }
                Err(_) => break,
            }
        }

        // For simplicity, break after first request (no keep-alive to upstream)
        break;
    }

    Ok(())
}

/// Handle a plain HTTP proxy request.
async fn handle_plain_http(
    mut buf_reader: BufReader<TcpStream>,
    request_line: &str,
    filter: Arc<UrlFilter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 3 {
        return Ok(());
    }

    let url = parts[1];
    // Extract host from URL
    let host = url.strip_prefix("http://")
        .and_then(|u| u.split('/').next())
        .unwrap_or("");
    let path = url.strip_prefix("http://")
        .and_then(|u| u.find('/').map(|i| &u[i..]))
        .unwrap_or("/");

    // Read headers
    let mut headers = Vec::new();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        if line.to_lowercase().starts_with("content-length:") {
            content_length = line.split(':').nth(1)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
        }
        headers.push(line);
    }

    if filter.should_block(host, path, url) {
        info!(%url, "Blocked HTTP request by URL filter");
        let response = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n";
        buf_reader.get_mut().write_all(response).await?;
        return Ok(());
    }

    // Forward to upstream
    let upstream_host = if host.contains(':') { host.to_string() } else { format!("{}:80", host) };
    let mut upstream = match TcpStream::connect(&upstream_host).await {
        Ok(s) => s,
        Err(_) => {
            let response = b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n";
            buf_reader.get_mut().write_all(response).await?;
            return Ok(());
        }
    };

    // Reconstruct request with relative path
    let reconstructed = format!("{} {} {}\r\n", parts[0], path, parts[2]);
    upstream.write_all(reconstructed.as_bytes()).await?;
    for h in &headers {
        upstream.write_all(h.as_bytes()).await?;
    }
    upstream.write_all(b"\r\n").await?;

    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        buf_reader.read_exact(&mut body).await?;
        upstream.write_all(&body).await?;
    }
    upstream.flush().await?;

    // Relay response back
    let mut stream = buf_reader.into_inner();
    tokio::io::copy(&mut upstream, &mut stream).await?;

    Ok(())
}

/// Get or create a TLS server config for the given domain.
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

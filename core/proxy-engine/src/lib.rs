//! FreeIX Transparent HTTPS Proxy Engine
//!
//! Intercepts HTTP/HTTPS traffic, applies URL-level ad filtering rules,
//! and blocks ad requests that DNS-level blocking cannot catch (e.g., YouTube ads).

pub mod filter;
pub mod proxy;
pub mod wfp;

use std::sync::Arc;
use thiserror::Error;
use tracing::info;

use freeix_ca::RootCa;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("proxy not running")]
    NotRunning,
    #[error("CA error: {0}")]
    Ca(#[from] freeix_ca::CaError),
}

pub struct ProxyEngine {
    ca: Arc<RootCa>,
    filter: Arc<filter::UrlFilter>,
    running: parking_lot::RwLock<bool>,
}

impl ProxyEngine {
    pub fn new(ca: Arc<RootCa>) -> Self {
        Self {
            ca,
            filter: Arc::new(filter::UrlFilter::new()),
            running: parking_lot::RwLock::new(false),
        }
    }

    pub fn filter(&self) -> &filter::UrlFilter {
        &self.filter
    }

    /// Start the transparent proxy on the given port.
    /// WFP will redirect traffic to this port.
    pub async fn start(&self, port: u16) -> Result<(), ProxyError> {
        if *self.running.read() {
            return Ok(());
        }

        let ca = self.ca.clone();
        let filter = self.filter.clone();

        tokio::spawn(async move {
            if let Err(e) = proxy::run_proxy(port, ca, filter).await {
                tracing::error!(error = %e, "proxy engine stopped with error");
            }
        });

        *self.running.write() = true;
        info!(port, "Proxy engine started");
        Ok(())
    }

    /// Enable WFP traffic redirection to our proxy.
    pub fn enable_wfp_redirect(&self, proxy_port: u16) -> Result<(), ProxyError> {
        wfp::enable_redirect(proxy_port)?;
        info!(proxy_port, "WFP redirect enabled");
        Ok(())
    }

    /// Disable WFP traffic redirection.
    pub fn disable_wfp_redirect(&self) -> Result<(), ProxyError> {
        wfp::disable_redirect()?;
        info!("WFP redirect disabled");
        Ok(())
    }
}

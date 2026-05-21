use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use freeix_dns_engine::DnsServer;
use freeix_filtering_engine::FilterEngine;
use freeix_platform::DnsBackup;
use parking_lot::RwLock;
use tokio::sync::broadcast;

/// Event emitted for every DNS query processed by the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEvent {
    pub timestamp: u64,
    pub domain: String,
    pub query_type: String,
    pub status: QueryStatus,
    pub response_time_ms: u32,
    pub upstream: String,
    pub rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum QueryStatus {
    Allowed,
    Blocked,
    Cached,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsProvider {
    pub id: String,
    pub name: String,
    pub primary: String,
    pub secondary: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub dns_provider_id: String,
    pub auto_start: bool,
    pub dark_mode: bool,
    pub cache_size: u32,
    pub listen_address: String,
    pub port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            dns_provider_id: "adguard".to_string(),
            auto_start: true,
            dark_mode: true,
            cache_size: 10000,
            listen_address: "127.0.0.1".to_string(),
            port: 53,
        }
    }
}

/// Live atomic stats — updated by the DNS handler, read by the UI.
pub struct LiveStats {
    pub total_queries: AtomicU64,
    pub blocked_queries: AtomicU64,
    pub cache_hits: AtomicU64,
    pub started_at: RwLock<Option<Instant>>,
}

impl LiveStats {
    pub fn new() -> Self {
        Self {
            total_queries: AtomicU64::new(0),
            blocked_queries: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            started_at: RwLock::new(None),
        }
    }

    pub fn record_query(&self, blocked: bool, cached: bool) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        if blocked {
            self.blocked_queries.fetch_add(1, Ordering::Relaxed);
        }
        if cached {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.started_at
            .read()
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }
}

pub struct AppState {
    pub protection_enabled: RwLock<bool>,
    pub config: RwLock<AppConfig>,
    pub stats: Arc<LiveStats>,
    pub whitelist: RwLock<HashSet<String>>,
    pub blacklist: RwLock<HashSet<String>>,
    pub query_log: RwLock<Vec<QueryEvent>>,
    pub dns_providers: Vec<DnsProvider>,
    pub dns_server: RwLock<Option<Arc<DnsServer>>>,
    pub filter_engine: Arc<FilterEngine>,
    pub dns_backup: RwLock<Option<DnsBackup>>,
    pub event_tx: broadcast::Sender<QueryEvent>,
}

impl AppState {
    pub fn new() -> Self {
        let dns_providers = vec![
            DnsProvider {
                id: "adguard".to_string(),
                name: "AdGuard DNS".to_string(),
                primary: "94.140.14.14".to_string(),
                secondary: "94.140.15.15".to_string(),
                description: "AdGuard DNS with built-in ad blocking".to_string(),
            },
            DnsProvider {
                id: "cloudflare-family".to_string(),
                name: "Cloudflare Family".to_string(),
                primary: "1.1.1.3".to_string(),
                secondary: "1.0.0.3".to_string(),
                description: "Cloudflare with malware and adult content blocking".to_string(),
            },
            DnsProvider {
                id: "mullvad".to_string(),
                name: "Mullvad DNS".to_string(),
                primary: "194.242.2.3".to_string(),
                secondary: "194.242.2.4".to_string(),
                description: "Mullvad privacy-focused ad-blocking DNS".to_string(),
            },
            DnsProvider {
                id: "quad9".to_string(),
                name: "Quad9".to_string(),
                primary: "9.9.9.9".to_string(),
                secondary: "149.112.112.112".to_string(),
                description: "Quad9 security-focused DNS with threat blocking".to_string(),
            },
            DnsProvider {
                id: "cloudflare".to_string(),
                name: "Cloudflare".to_string(),
                primary: "1.1.1.1".to_string(),
                secondary: "1.0.0.1".to_string(),
                description: "Cloudflare fast DNS (no filtering)".to_string(),
            },
        ];

        let (event_tx, _) = broadcast::channel(1000);

        Self {
            protection_enabled: RwLock::new(false),
            config: RwLock::new(AppConfig::default()),
            stats: Arc::new(LiveStats::new()),
            whitelist: RwLock::new(HashSet::new()),
            blacklist: RwLock::new(HashSet::new()),
            query_log: RwLock::new(Vec::with_capacity(10000)),
            dns_providers,
            dns_server: RwLock::new(None),
            filter_engine: Arc::new(FilterEngine::new()),
            dns_backup: RwLock::new(None),
            event_tx,
        }
    }

    pub fn push_log(&self, event: QueryEvent) {
        let mut logs = self.query_log.write();
        if logs.len() >= 10000 {
            logs.drain(0..5000); // Keep last 5000
        }
        logs.push(event);
    }
}

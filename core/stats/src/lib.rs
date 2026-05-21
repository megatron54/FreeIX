use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info};

#[derive(Debug, Error)]
pub enum StatsError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStats {
    pub total_queries: u64,
    pub blocked_queries: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub average_response_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStat {
    pub domain: String,
    pub query_count: u64,
    pub blocked_count: u64,
    pub last_queried: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLogEntry {
    pub timestamp: DateTime<Utc>,
    pub domain: String,
    pub query_type: String,
    pub blocked: bool,
    pub cached: bool,
    pub response_time_ms: u32,
    pub upstream: Option<String>,
}

pub struct StatsEngine {
    db: Mutex<Connection>,
    total_queries: AtomicU64,
    blocked_queries: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
}

impl StatsEngine {
    pub fn new(db_path: &Path) -> Result<Self, StatsError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS query_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                domain TEXT NOT NULL,
                query_type TEXT NOT NULL,
                blocked INTEGER NOT NULL DEFAULT 0,
                cached INTEGER NOT NULL DEFAULT 0,
                response_time_ms INTEGER NOT NULL DEFAULT 0,
                upstream TEXT
            );

            CREATE TABLE IF NOT EXISTS domain_stats (
                domain TEXT PRIMARY KEY,
                query_count INTEGER NOT NULL DEFAULT 0,
                blocked_count INTEGER NOT NULL DEFAULT 0,
                last_queried TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_query_log_timestamp ON query_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_query_log_domain ON query_log(domain);
            CREATE INDEX IF NOT EXISTS idx_domain_stats_count ON domain_stats(query_count DESC);
            ",
        )?;

        // Load counters from DB
        let total: u64 = conn
            .query_row("SELECT COUNT(*) FROM query_log", [], |r| r.get(0))
            .unwrap_or(0);
        let blocked: u64 = conn
            .query_row("SELECT COUNT(*) FROM query_log WHERE blocked = 1", [], |r| r.get(0))
            .unwrap_or(0);

        info!(path = %db_path.display(), total, blocked, "stats engine initialized");

        Ok(Self {
            db: Mutex::new(conn),
            total_queries: AtomicU64::new(total),
            blocked_queries: AtomicU64::new(blocked),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
        })
    }

    pub fn record_query(&self, entry: &QueryLogEntry) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        if entry.blocked {
            self.blocked_queries.fetch_add(1, Ordering::Relaxed);
        }
        if entry.cached {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        let db = self.db.lock();
        let timestamp = entry.timestamp.to_rfc3339();

        if let Err(e) = db.execute(
            "INSERT INTO query_log (timestamp, domain, query_type, blocked, cached, response_time_ms, upstream)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                timestamp,
                entry.domain,
                entry.query_type,
                entry.blocked as i32,
                entry.cached as i32,
                entry.response_time_ms,
                entry.upstream,
            ],
        ) {
            error!(error = %e, "failed to insert query log");
        }

        // Upsert domain stats
        if let Err(e) = db.execute(
            "INSERT INTO domain_stats (domain, query_count, blocked_count, last_queried)
             VALUES (?1, 1, ?2, ?3)
             ON CONFLICT(domain) DO UPDATE SET
               query_count = query_count + 1,
               blocked_count = blocked_count + ?2,
               last_queried = ?3",
            params![entry.domain, entry.blocked as i32, timestamp],
        ) {
            error!(error = %e, "failed to upsert domain stats");
        }
    }

    pub fn get_stats(&self) -> QueryStats {
        let total = self.total_queries.load(Ordering::Relaxed);
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);

        QueryStats {
            total_queries: total,
            blocked_queries: self.blocked_queries.load(Ordering::Relaxed),
            cache_hits: hits,
            cache_misses: misses,
            average_response_ms: 0.0, // Could compute from DB
        }
    }

    pub fn get_top_queried(&self, limit: usize) -> Vec<DomainStat> {
        let db = self.db.lock();
        let mut stmt = db
            .prepare(
                "SELECT domain, query_count, blocked_count, last_queried
                 FROM domain_stats ORDER BY query_count DESC LIMIT ?1",
            )
            .unwrap();

        stmt.query_map(params![limit as i64], |row| {
            let ts: String = row.get(3)?;
            let last_queried = DateTime::parse_from_rfc3339(&ts)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(DomainStat {
                domain: row.get(0)?,
                query_count: row.get(1)?,
                blocked_count: row.get(2)?,
                last_queried,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_top_blocked(&self, limit: usize) -> Vec<DomainStat> {
        let db = self.db.lock();
        let mut stmt = db
            .prepare(
                "SELECT domain, query_count, blocked_count, last_queried
                 FROM domain_stats WHERE blocked_count > 0
                 ORDER BY blocked_count DESC LIMIT ?1",
            )
            .unwrap();

        stmt.query_map(params![limit as i64], |row| {
            let ts: String = row.get(3)?;
            let last_queried = DateTime::parse_from_rfc3339(&ts)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(DomainStat {
                domain: row.get(0)?,
                query_count: row.get(1)?,
                blocked_count: row.get(2)?,
                last_queried,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Delete logs older than the given number of days.
    pub fn cleanup(&self, retention_days: u32) -> Result<usize, StatsError> {
        let db = self.db.lock();
        let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let deleted = db.execute(
            "DELETE FROM query_log WHERE timestamp < ?1",
            params![cutoff_str],
        )?;

        info!(deleted, retention_days, "cleaned up old stats");
        Ok(deleted)
    }
}

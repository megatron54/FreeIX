use std::collections::HashMap;
use std::time::{Duration, Instant};

use hickory_proto::op::Message;
use parking_lot::RwLock;
use tracing::debug;

/// Key for cache entries: (domain, record_type)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CacheKey {
    pub name: String,
    pub record_type: u16,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub response: Message,
    pub inserted_at: Instant,
    pub ttl: Duration,
    pub last_accessed: Instant,
}

impl CacheEntry {
    pub fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size: usize,
}

pub struct DnsCache {
    entries: RwLock<HashMap<CacheKey, CacheEntry>>,
    max_entries: usize,
    stats: RwLock<CacheStats>,
}

impl DnsCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::with_capacity(max_entries / 2)),
            max_entries,
            stats: RwLock::new(CacheStats::default()),
        }
    }

    pub fn get(&self, key: &CacheKey) -> Option<Message> {
        let entries = self.entries.read();
        if let Some(entry) = entries.get(key) {
            if entry.is_expired() {
                drop(entries);
                self.entries.write().remove(key);
                self.stats.write().misses += 1;
                return None;
            }
            self.stats.write().hits += 1;
            debug!(name = %key.name, "cache hit");
            Some(entry.response.clone())
        } else {
            self.stats.write().misses += 1;
            None
        }
    }

    pub fn insert(&self, key: CacheKey, response: Message, ttl: Duration) {
        let mut entries = self.entries.write();

        // Evict expired entries if at capacity
        if entries.len() >= self.max_entries {
            self.evict_expired(&mut entries);
        }

        // If still at capacity, evict oldest accessed
        if entries.len() >= self.max_entries {
            self.evict_lru(&mut entries);
        }

        let entry = CacheEntry {
            response,
            inserted_at: Instant::now(),
            ttl,
            last_accessed: Instant::now(),
        };

        entries.insert(key, entry);
        self.stats.write().size = entries.len();
    }

    pub fn clear(&self) {
        self.entries.write().clear();
        let mut stats = self.stats.write();
        stats.size = 0;
    }

    pub fn stats(&self) -> CacheStats {
        self.stats.read().clone()
    }

    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    fn evict_expired(&self, entries: &mut HashMap<CacheKey, CacheEntry>) {
        let before = entries.len();
        entries.retain(|_, entry| !entry.is_expired());
        let evicted = before - entries.len();
        if evicted > 0 {
            self.stats.write().evictions += evicted as u64;
            debug!(evicted, "evicted expired cache entries");
        }
    }

    fn evict_lru(&self, entries: &mut HashMap<CacheKey, CacheEntry>) {
        // Remove the 25% oldest entries by last_accessed
        let target = self.max_entries / 4;
        let mut items: Vec<_> = entries.iter().map(|(k, v)| (k.clone(), v.last_accessed)).collect();
        items.sort_by_key(|(_, t)| *t);

        for (key, _) in items.into_iter().take(target) {
            entries.remove(&key);
        }
        self.stats.write().evictions += target as u64;
    }
}

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// A generic thread-safe LRU cache with TTL-based expiration.
pub struct LruCache<K, V> {
    inner: Mutex<CacheInner<K, V>>,
}

struct CacheInner<K, V> {
    entries: HashMap<K, Entry<V>>,
    max_capacity: usize,
    order: Vec<K>, // Most recently used at the end
    hits: u64,
    misses: u64,
}

struct Entry<V> {
    value: V,
    inserted_at: Instant,
    ttl: Duration,
}

impl<V> Entry<V> {
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
    pub hits: u64,
    pub misses: u64,
}

impl<K: Hash + Eq + Clone, V: Clone> LruCache<K, V> {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                entries: HashMap::with_capacity(max_capacity),
                max_capacity,
                order: Vec::with_capacity(max_capacity),
                hits: 0,
                misses: 0,
            }),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock();

        let expired = inner.entries.get(key).map(|e| e.is_expired());
        match expired {
            Some(true) => {
                inner.entries.remove(key);
                inner.order.retain(|k| k != key);
                inner.misses += 1;
                None
            }
            Some(false) => {
                let value = inner.entries.get(key).unwrap().value.clone();
                inner.order.retain(|k| k != key);
                inner.order.push(key.clone());
                inner.hits += 1;
                Some(value)
            }
            None => {
                inner.misses += 1;
                None
            }
        }
    }

    pub fn insert(&self, key: K, value: V, ttl: Duration) {
        let mut inner = self.inner.lock();

        // Remove if exists
        if inner.entries.contains_key(&key) {
            inner.order.retain(|k| k != &key);
        }

        // Evict if at capacity
        while inner.entries.len() >= inner.max_capacity {
            if let Some(evict_key) = inner.order.first().cloned() {
                inner.entries.remove(&evict_key);
                inner.order.remove(0);
            } else {
                break;
            }
        }

        inner.order.push(key.clone());
        inner.entries.insert(
            key,
            Entry {
                value,
                inserted_at: Instant::now(),
                ttl,
            },
        );
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.lock();
        inner.order.retain(|k| k != key);
        inner.entries.remove(key).map(|e| e.value)
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.entries.clear();
        inner.order.clear();
    }

    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().entries.is_empty()
    }

    pub fn stats(&self) -> CacheStats {
        let inner = self.inner.lock();
        CacheStats {
            size: inner.entries.len(),
            capacity: inner.max_capacity,
            hits: inner.hits,
            misses: inner.misses,
        }
    }

    /// Remove all expired entries.
    pub fn evict_expired(&self) -> usize {
        let mut inner = self.inner.lock();
        let before = inner.entries.len();
        let expired_keys: Vec<K> = inner
            .entries
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for key in &expired_keys {
            inner.entries.remove(key);
            inner.order.retain(|k| k != key);
        }
        before - inner.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let cache: LruCache<String, i32> = LruCache::new(3);
        cache.insert("a".into(), 1, Duration::from_secs(60));
        cache.insert("b".into(), 2, Duration::from_secs(60));
        cache.insert("c".into(), 3, Duration::from_secs(60));

        assert_eq!(cache.get(&"a".into()), Some(1));
        assert_eq!(cache.get(&"d".into()), None);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_eviction() {
        let cache: LruCache<String, i32> = LruCache::new(2);
        cache.insert("a".into(), 1, Duration::from_secs(60));
        cache.insert("b".into(), 2, Duration::from_secs(60));
        cache.insert("c".into(), 3, Duration::from_secs(60)); // evicts "a"

        assert_eq!(cache.get(&"a".into()), None);
        assert_eq!(cache.get(&"c".into()), Some(3));
    }

    #[test]
    fn test_ttl_expiry() {
        let cache: LruCache<String, i32> = LruCache::new(10);
        cache.insert("x".into(), 42, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(cache.get(&"x".into()), None);
    }
}

use std::collections::HashMap;
use std::time::{Duration, Instant};

struct CacheEntry {
    value: serde_json::Value,
    inserted_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

pub struct MemoryCache {
    entries: HashMap<String, CacheEntry>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.entries.get(key).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(&entry.value)
            }
        })
    }

    pub fn put(&mut self, key: String, value: serde_json::Value, ttl: Duration) {
        self.entries.insert(
            key,
            CacheEntry {
                value,
                inserted_at: Instant::now(),
                ttl,
            },
        );
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Remove expired entries
    pub fn evict_expired(&mut self) {
        self.entries.retain(|_, entry| !entry.is_expired());
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

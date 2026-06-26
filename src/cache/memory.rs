use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;
use chrono::Utc;
use anyhow::Result;

use super::{CacheBackend, CacheEntry};

pub struct MemoryCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    max_bytes: usize,
}

impl MemoryCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_bytes,
        }
    }
}

impl CacheBackend for MemoryCache {
    fn insert(&self, id: &str, content: &str, session: Option<&str>) -> Result<()> {
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        
        // Eviction logic
        let mut total_size: usize = cache.values().map(|entry| entry.content.len()).sum();
        while total_size + content.len() > self.max_bytes && !cache.is_empty() {
            let mut oldest_key = None;
            let mut oldest_time = Instant::now();
            
            for (k, entry) in cache.iter() {
                let entry_last_accessed = match entry.last_accessed.lock() {
                    Ok(guard) => *guard,
                    Err(poisoned) => *poisoned.into_inner(),
                };
                if entry_last_accessed < oldest_time {
                    oldest_time = entry_last_accessed;
                    oldest_key = Some(k.clone());
                }
            }
            
            if let Some(k) = oldest_key {
                if let Some(removed) = cache.remove(&k) {
                    total_size -= removed.content.len();
                }
            } else {
                break;
            }
        }

        let created_at = Utc::now().to_rfc3339();
        cache.insert(
            id.to_string(),
            CacheEntry {
                content: content.to_string(),
                last_accessed: std::sync::Mutex::new(Instant::now()),
                session: session.map(String::from),
                created_at,
            },
        );
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<String>> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = cache.get(id) {
            let mut last = match entry.last_accessed.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            *last = Instant::now();
            Ok(Some(entry.content.clone()))
        } else {
            Ok(None)
        }
    }

    fn remove(&self, id: &str) -> Result<Option<usize>> {
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = cache.remove(id) {
            Ok(Some(entry.content.len()))
        } else {
            Ok(None)
        }
    }

    fn clear(&self) -> Result<(usize, usize)> {
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        let count = cache.len();
        let total_bytes: usize = cache.values().map(|entry| entry.content.len()).sum();
        cache.clear();
        Ok((count, total_bytes))
    }

    fn stats(&self) -> Result<Vec<(String, usize)>> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        let mut stats = Vec::new();
        for (k, entry) in cache.iter() {
            stats.push((k.clone(), entry.content.len()));
        }
        Ok(stats)
    }

    fn search(&self, query: &str) -> Result<Vec<(String, String)>> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for (id, entry) in cache.iter() {
            if entry.content.to_lowercase().contains(&query_lower) {
                let content = &entry.content;
                let snippet = if let Some(idx) = content.to_lowercase().find(&query_lower) {
                    let start = idx.saturating_sub(30);
                    let end = (idx + query.len() + 50).min(content.len());
                    // extract string slice safely taking char boundaries into account
                    let sub = &content[start..end];
                    format!("...{}...", sub.replace('\n', " "))
                } else {
                    content.chars().take(80).collect::<String>()
                };
                results.push((id.clone(), snippet));
            }
        }
        Ok(results)
    }

    fn total_bytes(&self) -> Result<usize> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        Ok(cache.values().map(|entry| entry.content.len()).sum())
    }

    fn len(&self) -> Result<usize> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        Ok(cache.len())
    }

    fn expire_old(&self, max_age_hours: u64) -> Result<usize> {
        if max_age_hours == 0 {
            return Ok(0);
        }
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        let mut to_remove = Vec::new();
        for (k, entry) in cache.iter() {
            let last_accessed = match entry.last_accessed.lock() {
                Ok(guard) => *guard,
                Err(poisoned) => *poisoned.into_inner(),
            };
            if last_accessed.elapsed().as_secs() > max_age_hours * 3600 {
                to_remove.push(k.clone());
            }
        }
        let count = to_remove.len();
        for k in to_remove {
            cache.remove(&k);
        }
        Ok(count)
    }

    fn export_all(&self) -> Result<Vec<(String, String, Option<String>, String)>> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        let mut results = Vec::new();
        for (id, entry) in cache.iter() {
            results.push((id.clone(), entry.content.clone(), entry.session.clone(), entry.created_at.clone()));
        }
        Ok(results)
    }
}

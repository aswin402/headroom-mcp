pub mod entry;
pub mod memory;
pub mod sqlite;

use anyhow::Result;
pub use entry::CacheEntry;

pub trait CacheBackend: Send + Sync {
    fn insert(&self, id: &str, content: &str, session: Option<&str>) -> Result<()>;
    fn get(&self, id: &str) -> Result<Option<String>>;
    fn remove(&self, id: &str) -> Result<Option<usize>>;
    fn clear(&self) -> Result<(usize, usize)>; // returns (evicted_count, freed_bytes)
    fn stats(&self) -> Result<Vec<(String, usize)>>; // returns list of (ccr_id, size_bytes)
    fn search(&self, query: &str) -> Result<Vec<(String, String)>>; // returns list of (ccr_id, snippet/content)
    fn total_bytes(&self) -> Result<usize>;
    fn len(&self) -> Result<usize>;
    fn expire_old(&self, max_age_hours: u64) -> Result<usize>;
    fn export_all(&self) -> Result<Vec<(String, String, Option<String>, String)>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::memory::MemoryCache;
    use super::sqlite::SqliteCache;

    #[test]
    fn test_memory_cache() -> Result<()> {
        let cache = MemoryCache::new(100);
        cache.insert("ccr_1", "hello", Some("session_1"))?;
        assert_eq!(cache.len()?, 1);
        assert_eq!(cache.get("ccr_1")?, Some("hello".to_string()));
        
        let stats = cache.stats()?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].0, "ccr_1");
        assert_eq!(stats[0].1, 5);
        
        let search = cache.search("ell")?;
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].0, "ccr_1");
        
        // Eviction test
        let cache_small = MemoryCache::new(10);
        cache_small.insert("ccr_a", "12345", None)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache_small.insert("ccr_b", "67890", None)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache_small.insert("ccr_c", "abc", None)?;
        
        assert_eq!(cache_small.get("ccr_a")?, None);
        assert_eq!(cache_small.get("ccr_b")?, Some("67890".to_string()));
        assert_eq!(cache_small.get("ccr_c")?, Some("abc".to_string()));

        Ok(())
    }

    #[test]
    fn test_sqlite_cache() -> Result<()> {
        let db_path = "/tmp/test_headroom_mcp_cache.db";
        let _ = std::fs::remove_file(db_path);
        
        let cache = SqliteCache::open(db_path, 100)?;
        cache.insert("ccr_1", "hello", Some("session_1"))?;
        assert_eq!(cache.len()?, 1);
        assert_eq!(cache.get("ccr_1")?, Some("hello".to_string()));
        
        let stats = cache.stats()?;
        assert_eq!(stats.len(), 1);
        
        let search = cache.search("ell")?;
        assert_eq!(search.len(), 1);
        
        // Eviction test
        let db_path_small = "/tmp/test_headroom_mcp_cache_small.db";
        let _ = std::fs::remove_file(db_path_small);
        let cache_small = SqliteCache::open(db_path_small, 10)?;
        cache_small.insert("ccr_a", "12345", None)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache_small.insert("ccr_b", "67890", None)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache_small.insert("ccr_c", "abc", None)?;
        
        assert_eq!(cache_small.get("ccr_a")?, None);
        assert_eq!(cache_small.get("ccr_b")?, Some("67890".to_string()));
        assert_eq!(cache_small.get("ccr_c")?, Some("abc".to_string()));
        
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(db_path_small);
        Ok(())
    }
}

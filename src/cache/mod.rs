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
    #[allow(dead_code)]
    fn expire_old(&self, max_age_hours: u64) -> Result<usize>;
    fn export_all(&self) -> Result<Vec<(String, String, Option<String>, String)>>;
    fn log_compression(&self, _tool: &str, _orig_bytes: usize, _comp_bytes: usize,
                       _orig_tokens: usize, _comp_tokens: usize,
                       _content_type: &str, _model_hint: Option<&str>) -> Result<()> {
        Ok(())
    }
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

    #[test]
    fn test_sqlite_analytics() -> Result<()> {
        let db_path = "/tmp/test_headroom_mcp_analytics.db";
        let _ = std::fs::remove_file(db_path);
        
        let cache = SqliteCache::open(db_path, 100_000)?;
        
        // P16.T3: Verify table created
        {
            let conn = rusqlite::Connection::open(db_path)?;
            let mut stmt = conn.prepare("PRAGMA table_info(compression_log)")?;
            let mut rows = stmt.query([])?;
            let mut has_columns = false;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name == "tool_name" || name == "original_bytes" {
                    has_columns = true;
                }
            }
            assert!(has_columns);
        }

        // P16.T4: log_compression inserts a row
        cache.log_compression("compress_content", 1000, 300, 200, 60, "code", Some("claude-sonnet-4"))?;
        cache.log_compression("run_and_compress", 500, 250, 100, 50, "logs", Some("gpt-4o"))?;
        
        {
            let conn = rusqlite::Connection::open(db_path)?;
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM compression_log", [], |r| r.get(0))?;
            assert_eq!(count, 2);
        }

        // P16.T5: query_stats aggregates correctly
        let stats = cache.query_stats()?;
        assert_eq!(stats.total_compressions, 2);
        assert_eq!(stats.total_original_bytes, 1500);
        assert_eq!(stats.total_compressed_bytes, 550);
        assert_eq!(stats.total_original_tokens, 300);
        assert_eq!(stats.total_compressed_tokens, 110);

        // P16.T6: query_usage with filter
        let usage_filtered = cache.query_usage(Some("claude-sonnet-4"))?;
        assert_eq!(usage_filtered.len(), 1);
        assert_eq!(usage_filtered[0].model, "claude-sonnet-4");
        assert_eq!(usage_filtered[0].total_original_tokens, 200);
        assert_eq!(usage_filtered[0].total_saved_tokens, 140);

        // P16.T7: query_usage without filter
        let usage_all = cache.query_usage(None)?;
        assert_eq!(usage_all.len(), 2);

        // P16.T8 & P16.T9: Pricing functions
        assert_eq!(crate::intelligence::pricing::estimate_cost_usd("claude-sonnet-4", 1_000_000), 3.0);
        let default_price = crate::intelligence::pricing::get_price("unknown-model");
        assert_eq!(default_price.name, "default");

        // P16.T10: print_usage and print_stats execution (no panic)
        crate::analytics::print_stats(db_path)?;
        crate::analytics::print_usage(db_path, None, false)?;
        crate::analytics::print_usage(db_path, None, true)?;

        let _ = std::fs::remove_file(db_path);
        Ok(())
    }
}

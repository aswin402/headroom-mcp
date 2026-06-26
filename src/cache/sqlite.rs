use std::sync::Mutex;
use rusqlite::Connection;
use chrono::Utc;
use anyhow::Result;

use super::CacheBackend;

pub struct SqliteCache {
    conn: Mutex<Connection>,
    max_bytes: usize,
}

impl SqliteCache {
    pub fn open(path: &str, max_bytes: usize) -> Result<Self> {
        let conn = Connection::open(path)?;
        
        // Enable WAL mode for concurrency and speed
        let _ = conn.execute("PRAGMA journal_mode = WAL", []);
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_entries (
                ccr_id      TEXT PRIMARY KEY,
                content     TEXT NOT NULL,
                session     TEXT,
                created_at  TEXT NOT NULL,
                accessed_at TEXT NOT NULL,
                size_bytes  INTEGER NOT NULL
            )",
            [],
        )?;

        // FTS5 virtual table for full-text search
        let _ = conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS cache_fts USING fts5(
                ccr_id UNINDEXED,
                content,
                tokenize = 'porter unicode61'
            )",
            [],
        );

        Ok(Self {
            conn: Mutex::new(conn),
            max_bytes,
        })
    }
}

impl CacheBackend for SqliteCache {
    fn insert(&self, id: &str, content: &str, session: Option<&str>) -> Result<()> {
        // Run eviction first
        let mut total_size = self.total_bytes()?;
        while total_size + content.len() > self.max_bytes {
            let oldest: Option<(String, usize)> = {
                let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
                conn.query_row(
                    "SELECT ccr_id, size_bytes FROM cache_entries ORDER BY accessed_at ASC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                ).ok()
            };
            if let Some((old_id, size)) = oldest {
                self.remove(&old_id)?;
                total_size -= size;
            } else {
                break;
            }
        }

        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        
        conn.execute(
            "INSERT OR REPLACE INTO cache_entries (ccr_id, content, session, created_at, accessed_at, size_bytes)
             VALUES (?, ?, ?, ?, ?, ?)",
            (id, content, session, &now, &now, content.len()),
        )?;

        // Update FTS5 virtual table
        let _ = conn.execute("DELETE FROM cache_fts WHERE ccr_id = ?", [id]);
        let _ = conn.execute("INSERT INTO cache_fts (ccr_id, content) VALUES (?, ?)", (id, content));

        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let res: Option<String> = conn.query_row(
            "SELECT content FROM cache_entries WHERE ccr_id = ?",
            [id],
            |row| row.get(0),
        ).ok();

        if res.is_some() {
            let now = Utc::now().to_rfc3339();
            let _ = conn.execute(
                "UPDATE cache_entries SET accessed_at = ? WHERE ccr_id = ?",
                (&now, id),
            );
        }
        Ok(res)
    }

    fn remove(&self, id: &str) -> Result<Option<usize>> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let size: Option<usize> = conn.query_row(
            "SELECT size_bytes FROM cache_entries WHERE ccr_id = ?",
            [id],
            |row| row.get(0),
        ).ok();

        if size.is_some() {
            let _ = conn.execute("DELETE FROM cache_entries WHERE ccr_id = ?", [id]);
            let _ = conn.execute("DELETE FROM cache_fts WHERE ccr_id = ?", [id]);
        }
        Ok(size)
    }

    fn clear(&self) -> Result<(usize, usize)> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let count: usize = conn.query_row("SELECT COUNT(*) FROM cache_entries", [], |row| row.get(0))?;
        let bytes: usize = conn.query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM cache_entries", [], |row| row.get(0))?;
        
        let _ = conn.execute("DELETE FROM cache_entries", []);
        let _ = conn.execute("DELETE FROM cache_fts", []);
        Ok((count, bytes))
    }

    fn stats(&self) -> Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare("SELECT ccr_id, size_bytes FROM cache_entries")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?)))?;
        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }

    fn search(&self, query: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        
        // Try FTS5 matching first
        let stmt = conn.prepare(
            "SELECT ccr_id, snippet(cache_fts, 1, '...', '...', '...', 10) 
             FROM cache_fts 
             WHERE cache_fts MATCH ? 
             ORDER BY rank LIMIT 10"
        );
        
        let results = match stmt {
            Ok(mut s) => {
                let formatted = format!("\"{}\"", query.replace('"', ""));
                let rows = s.query_map([formatted], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)));
                match rows {
                    Ok(r) => {
                        let mut res = Vec::new();
                        for item in r {
                            if let Ok(v) = item {
                                res.push(v);
                            }
                        }
                        res
                    }
                    Err(_) => Vec::new()
                }
            }
            Err(_) => Vec::new()
        };

        if !results.is_empty() {
            return Ok(results);
        }

        // Fallback LIKE search
        let mut stmt = conn.prepare(
            "SELECT ccr_id, content FROM cache_entries WHERE content LIKE ? LIMIT 10"
        )?;
        let rows = stmt.query_map([format!("%{}%", query)], |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let query_lower = query.to_lowercase();
            let snippet = if let Some(idx) = content.to_lowercase().find(&query_lower) {
                let start = idx.saturating_sub(30);
                let end = (idx + query.len() + 50).min(content.len());
                // Safe boundary check
                let start_idx = content.char_indices().map(|(i, _)| i).find(|&i| i >= start).unwrap_or(0);
                let end_idx = content.char_indices().map(|(i, _)| i).find(|&i| i >= end).unwrap_or(content.len());
                let sub = &content[start_idx..end_idx];
                format!("...{}...", sub.replace('\n', " "))
            } else {
                content.chars().take(80).collect::<String>()
            };
            Ok((id, snippet))
        })?;
        
        let mut fallback_results = Vec::new();
        for r in rows {
            fallback_results.push(r?);
        }
        Ok(fallback_results)
    }

    fn total_bytes(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let res: usize = conn.query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM cache_entries", [], |row| row.get(0))?;
        Ok(res)
    }

    fn len(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let res: usize = conn.query_row("SELECT COUNT(*) FROM cache_entries", [], |row| row.get(0))?;
        Ok(res)
    }

    fn expire_old(&self, max_age_hours: u64) -> Result<usize> {
        if max_age_hours == 0 {
            return Ok(0);
        }
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let threshold_modifier = format!("-{} hours", max_age_hours);
        
        let to_remove: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT ccr_id FROM cache_entries 
                 WHERE accessed_at < datetime('now', ?)"
            )?;
            let rows = stmt.query_map([&threshold_modifier], |row| row.get::<_, String>(0))?;
            let mut ids = Vec::new();
            for r in rows {
                ids.push(r?);
            }
            ids
        };

        let count = to_remove.len();
        for id in to_remove {
            let _ = conn.execute("DELETE FROM cache_entries WHERE ccr_id = ?", [&id]);
            let _ = conn.execute("DELETE FROM cache_fts WHERE ccr_id = ?", [&id]);
        }
        Ok(count)
    }

    fn export_all(&self) -> Result<Vec<(String, String, Option<String>, String)>> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare("SELECT ccr_id, content, session, created_at FROM cache_entries")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }
}

use std::time::Instant;

pub struct CacheEntry {
    pub content: String,
    pub last_accessed: Instant,
}

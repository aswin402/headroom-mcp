use std::time::Instant;

#[derive(Clone)]
pub struct CacheEntry {
    pub content: String,
    pub last_accessed: Instant,
    pub session: Option<String>,
    pub created_at: String,
}

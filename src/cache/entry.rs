use std::time::Instant;
use std::sync::Mutex;

pub struct CacheEntry {
    pub content: String,
    pub last_accessed: Mutex<Instant>,
    pub session: Option<String>,
    pub created_at: String,
}

impl Clone for CacheEntry {
    fn clone(&self) -> Self {
        let last_val = match self.last_accessed.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        };
        Self {
            content: self.content.clone(),
            last_accessed: Mutex::new(last_val),
            session: self.session.clone(),
            created_at: self.created_at.clone(),
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

pub struct Metrics {
    pub compressions_total: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub total_bytes_compressed: AtomicU64,
    pub total_bytes_saved: AtomicU64,
    pub retrievals_total: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            compressions_total: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            total_bytes_compressed: AtomicU64::new(0),
            total_bytes_saved: AtomicU64::new(0),
            retrievals_total: AtomicU64::new(0),
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"compressions_total\":{},\"cache_hits\":{},\"cache_misses\":{},\"total_bytes_compressed\":{},\"total_bytes_saved\":{},\"retrievals_total\":{}}}",
            self.compressions_total.load(Ordering::Relaxed),
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
            self.total_bytes_compressed.load(Ordering::Relaxed),
            self.total_bytes_saved.load(Ordering::Relaxed),
            self.retrievals_total.load(Ordering::Relaxed)
        )
    }
}

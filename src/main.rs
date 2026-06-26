mod cache;
mod cli;
mod compression;
mod config;
mod intelligence;
mod server;
mod metrics;
mod tools;

use clap::Parser;
use rmcp::{transport::stdio, ServiceExt};
use std::sync::Arc;

use crate::cli::CliArgs;
use crate::config::Config;
use crate::server::HeadroomServer;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = CliArgs::parse();
    let config = Arc::new(Config::from_cli(args));
    
    let cache: Arc<dyn cache::CacheBackend> = if let Some(ref db_path) = config.db_path {
        match cache::sqlite::SqliteCache::open(db_path, config.max_cache_bytes) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                eprintln!("[Headroom MCP] [ERROR] Failed to open SQLite persistent DB at '{}': {}. Falling back to in-memory cache.", db_path, e);
                Arc::new(cache::memory::MemoryCache::new(config.max_cache_bytes))
            }
        }
    } else {
        Arc::new(cache::memory::MemoryCache::new(config.max_cache_bytes))
    };

    let metrics = Arc::new(metrics::Metrics::new());

    if config.metrics_interval > 0 {
        let metrics_cloned = metrics.clone();
        let interval_secs = config.metrics_interval;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            // First tick finishes immediately, so we should tick once before entering the loop
            // to avoid printing immediately on startup (or we can just let it print if we want).
            // Let's print periodically:
            loop {
                interval.tick().await;
                eprintln!("[Headroom MCP] [METRICS] {}", metrics_cloned.to_json());
            }
        });
    }

    let server = HeadroomServer::new(config, cache, metrics);

    // Start the stdio transport server
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

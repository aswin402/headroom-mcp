mod cache;
mod cli;
mod compression;
mod config;
mod intelligence;
mod server;
mod metrics;
mod tools;
mod analytics;

use clap::Parser;
use rmcp::{transport::stdio, ServiceExt};
use std::sync::Arc;

use crate::cli::CliArgs;
use crate::config::Config;
use crate::server::HeadroomServer;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = CliArgs::parse();

    if let Some(ref command) = args.command {
        match command {
            cli::Commands::Stats { db_path } => {
                let env_path = std::env::var("HEADROOM_DB_PATH").ok();
                let path = db_path
                    .as_deref()
                    .or(args.db_path.as_deref())
                    .or(env_path.as_deref())
                    .unwrap_or("");
                analytics::print_stats(path)?;
                return Ok(());
            }
            cli::Commands::Usage { db_path, model, json } => {
                let env_path = std::env::var("HEADROOM_DB_PATH").ok();
                let path = db_path
                    .as_deref()
                    .or(args.db_path.as_deref())
                    .or(env_path.as_deref())
                    .unwrap_or("");
                analytics::print_usage(path, model.as_deref(), *json)?;
                return Ok(());
            }
            cli::Commands::Serve => {}
        }
    }

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

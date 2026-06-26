mod cache;
mod cli;
mod compression;
mod config;
mod intelligence;
mod server;

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

    let server = HeadroomServer::new(config, cache);

    // Start the stdio transport server
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

mod cache;
mod compression;
mod config;
mod server;

use rmcp::{transport::stdio, ServiceExt};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::server::HeadroomServer;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cache = Arc::new(Mutex::new(HashMap::new()));
    let server = HeadroomServer::new(cache);

    // Start the stdio transport server
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

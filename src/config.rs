#![allow(dead_code)]

use crate::cli::CliArgs;

pub const DEFAULT_LOG_THRESHOLD: usize = 50_000;
pub const DEFAULT_JSON_THRESHOLD: usize = 10_000;
pub const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024; // 10 MB limit
pub const MAX_CACHE_BYTES: usize = 100 * 1024 * 1024; // 100 MB cache limit
pub const SCOPE_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CURSOR.md", ".cursorrules"];

#[derive(Debug, Clone)]
pub struct Config {
    pub log_threshold: usize,
    pub json_threshold: usize,
    pub max_input_size: usize,
    pub max_cache_bytes: usize,
    pub workspace_root: Option<String>,
    pub db_path: Option<String>,
    pub cache_ttl_hours: u64,
    pub metrics_interval: u64,
}

impl Config {
    pub fn from_cli(args: CliArgs) -> Self {
        Self {
            log_threshold: args.log_threshold,
            json_threshold: args.json_threshold,
            max_input_size: args.max_input_size,
            max_cache_bytes: args.max_cache_bytes,
            workspace_root: args.workspace_root,
            db_path: args.db_path,
            cache_ttl_hours: args.cache_ttl_hours,
            metrics_interval: args.metrics_interval,
        }
    }
}

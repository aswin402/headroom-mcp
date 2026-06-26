use clap::Parser;

/// Headroom MCP — Context compression server for AI coding agents
#[derive(Parser, Debug, Clone)]
#[command(name = "headroom-mcp", version, about)]
pub struct CliArgs {
    /// Log compression threshold in characters
    #[arg(long, env = "HEADROOM_LOG_THRESHOLD", default_value_t = 50_000)]
    pub log_threshold: usize,

    /// JSON compression threshold in characters
    #[arg(long, env = "HEADROOM_JSON_THRESHOLD", default_value_t = 10_000)]
    pub json_threshold: usize,

    /// Maximum input size in bytes (default: 10MB)
    #[arg(long, env = "HEADROOM_MAX_INPUT", default_value_t = 10 * 1024 * 1024)]
    pub max_input_size: usize,

    /// Maximum cache size in bytes (default: 100MB)
    #[arg(long, env = "HEADROOM_MAX_CACHE_MB", default_value_t = 100 * 1024 * 1024)]
    pub max_cache_bytes: usize,

    /// Workspace root directory (default: current directory)
    #[arg(long, env = "HEADROOM_WORKSPACE")]
    pub workspace_root: Option<String>,

    /// SQLite database path for persistent cache (default: in-memory only)
    #[arg(long, env = "HEADROOM_DB_PATH")]
    pub db_path: Option<String>,

    /// Cache entry TTL in hours (0 = no expiry)
    #[arg(long, env = "HEADROOM_CACHE_TTL_HOURS", default_value_t = 0)]
    pub cache_ttl_hours: u64,

    /// Metrics reporting interval in seconds (0 = disabled)
    #[arg(long, env = "HEADROOM_METRICS_INTERVAL", default_value_t = 0)]
    pub metrics_interval: u64,
}

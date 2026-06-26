pub const DEFAULT_LOG_THRESHOLD: usize = 50_000;
pub const DEFAULT_JSON_THRESHOLD: usize = 10_000;
pub const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024; // 10 MB limit
pub const MAX_CACHE_BYTES: usize = 100 * 1024 * 1024; // 100 MB cache limit
pub const SCOPE_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CURSOR.md", ".cursorrules"];

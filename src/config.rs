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
    pub compact_schemas: bool,
    pub enforce_yagni: bool,
}

pub const YAGNI_DIRECTIVES: &str = r#"
---
### Headroom: YAGNI Minimalism Directives

Before writing ANY code, walk down this ladder and stop at the FIRST rung that applies:

1. **Does this need to exist?** → If no: skip it entirely (YAGNI).
2. **Already in this codebase?** → Reuse it. Do not rewrite.
3. **Standard library does it?** → Use std. No external crate/package.
4. **Native platform feature?** → Use it (e.g., `<input type="date">` over a date-picker library).
5. **Already-installed dependency does it?** → Use what's there. Don't add a new dep.
6. **Can it be one line?** → Write one line.
7. **Only then:** Implement the minimum that works.

**Never skip:** validation, error handling, security checks, accessibility.
The code should be small because it is *necessary*, not golfed.
---
"#;

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
            compact_schemas: args.compact_schemas,
            enforce_yagni: args.enforce_yagni,
        }
    }
}


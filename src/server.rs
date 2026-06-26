use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::cache::CacheEntry;
use crate::config::{
    DEFAULT_JSON_THRESHOLD, DEFAULT_LOG_THRESHOLD, MAX_CACHE_BYTES, MAX_INPUT_SIZE, SCOPE_FILES,
};
use crate::compression::{
    auto_detect_content_type, compress_code, compress_csv, compress_json, compress_logs,
    detect_content_type_from_ext,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct HeadroomServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    pub cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
    pub start_time: Instant,
}

// --- Request Structs ---
#[derive(Deserialize, JsonSchema)]
pub struct ScopeContextRequest {
    #[schemars(
        description = "Absolute or relative path to the file/directory the agent is editing."
    )]
    pub target_path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompressContentRequest {
    #[schemars(description = "The raw string content to compress.")]
    pub raw_text: String,
    #[schemars(description = "The content type: 'json', 'code', 'text_logs', 'csv', 'markdown', 'yaml', or 'auto'.")]
    pub content_type: String,
    #[schemars(description = "Optional compression threshold override.")]
    pub threshold: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RetrieveOriginalRequest {
    #[schemars(description = "The CCR ID (e.g. ccr_a1b2c) to retrieve.")]
    pub ccr_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompressFileRequest {
    #[schemars(description = "Path to the file to compress.")]
    pub file_path: String,
    #[schemars(description = "Optional content type override: 'json', 'code', 'text_logs', 'csv', 'markdown', 'yaml', or 'auto'.")]
    pub content_type: Option<String>,
    #[schemars(description = "Optional compression threshold override.")]
    pub threshold: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CacheStatsRequest {}

#[derive(Deserialize, JsonSchema)]
pub struct ClearCacheRequest {}

#[derive(Deserialize, JsonSchema)]
pub struct ServerInfoRequest {}

fn mcp_error<E: std::fmt::Display>(err: E) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(err.to_string(), None)
}

fn log_info(msg: &str) {
    eprintln!("[Headroom MCP] [INFO] {}", msg);
}

fn log_error(msg: &str) {
    eprintln!("[Headroom MCP] [ERROR] {}", msg);
}

#[tool_router(server_handler)]
impl HeadroomServer {
    pub fn new(cache: Arc<Mutex<HashMap<String, CacheEntry>>>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            cache,
            start_time: Instant::now(),
        }
    }

    #[tool(
        description = "Walks up the directory tree and retrieves all relevant context files (AGENTS.md, CLAUDE.md, etc.) for the target file path."
    )]
    async fn scope_context(
        &self,
        req: Parameters<ScopeContextRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        log_info(&format!("scope_context: {}", req.0.target_path));
        let path = Path::new(&req.0.target_path);
        
        let workspace_root = std::env::current_dir()
            .map_err(mcp_error)?
            .canonicalize()
            .map_err(mcp_error)?;

        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        };

        // Resolve absolute path to canonical if possible
        let resolved_path = absolute_path.canonicalize().map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Failed to resolve path '{}': {}", req.0.target_path, e),
                None,
            )
        })?;

        // Ensure we don't scope files outside workspace root
        if !resolved_path.starts_with(&workspace_root) {
            log_error(&format!("Access denied for path: {}", req.0.target_path));
            return Err(rmcp::ErrorData::internal_error(
                format!("Access denied: path '{}' is outside workspace root", req.0.target_path),
                None,
            ));
        }

        let target_dir = if resolved_path.is_dir() {
            resolved_path.as_path()
        } else {
            resolved_path.parent().unwrap_or(&workspace_root)
        };

        let mut agents_files = Vec::new();
        let mut current_dir = Some(target_dir);

        while let Some(dir) = current_dir {
            for filename in SCOPE_FILES {
                let file_path = dir.join(filename);
                if file_path.is_file() {
                    agents_files.push(file_path);
                }
            }

            // Stop at git repository root, workspace root, or filesystem root
            if dir.join(".git").exists() || dir == workspace_root {
                break;
            }

            current_dir = dir.parent();
        }

        // Combine from root/parent down to target directory
        agents_files.reverse();

        if agents_files.is_empty() {
            return Ok("No context files (AGENTS.md, CLAUDE.md, etc.) found in the path hierarchy.".to_string());
        }

        let mut combined_content = String::new();
        for file_path in agents_files {
            let content = fs::read_to_string(&file_path).map_err(mcp_error)?;
            let relative_path = file_path
                .strip_prefix(&workspace_root)
                .unwrap_or(&file_path);
            combined_content.push_str(&format!(
                "### Context File: {}\n\n{}\n\n",
                relative_path.display(),
                content
            ));
        }

        Ok(combined_content)
    }

    #[tool(
        description = "Compresses logs, JSON, or code, and registers a CCR reference token for the agent."
    )]
    async fn compress_content(
        &self,
        req: Parameters<CompressContentRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        let raw_text = req.0.raw_text.trim();
        if raw_text.is_empty() {
            return Ok("Empty content provided.".to_string());
        }

        // Max input size check
        if raw_text.len() > MAX_INPUT_SIZE {
            return Err(rmcp::ErrorData::internal_error(
                format!("Content size exceeds maximum allowed size of {} bytes", MAX_INPUT_SIZE),
                None,
            ));
        }

        let content_type = req.0.content_type.to_lowercase();
        let content_type_ref = if content_type == "auto" || content_type.is_empty() {
            auto_detect_content_type(raw_text)
        } else {
            match content_type.as_str() {
                "json" | "code" | "text_logs" | "csv" | "markdown" | "yaml" => content_type.as_str(),
                other => return Err(rmcp::ErrorData::internal_error(
                    format!("Unknown content_type '{}'. Use 'json', 'code', 'text_logs', 'csv', 'markdown', 'yaml', or 'auto'.", other),
                    None,
                )),
            }
        };

        log_info(&format!("compress_content: type={}", content_type_ref));

        let threshold = req.0.threshold;
        let compressed = match content_type_ref {
            "json" => compress_json(raw_text, threshold.unwrap_or(DEFAULT_JSON_THRESHOLD)).map_err(mcp_error)?,
            "code" | "yaml" | "markdown" => compress_code(raw_text),
            "csv" => compress_csv(raw_text),
            _ => compress_logs(raw_text, threshold.unwrap_or(DEFAULT_LOG_THRESHOLD)),
        };

        // Generate highly unique CCR ID using timestamp and atomic counter
        let time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let ccr_id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);

        // Cache only after successful compression, with LRU eviction
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            let mut total_size: usize = cache.values().map(|entry| entry.content.len()).sum();
            
            while total_size + raw_text.len() > MAX_CACHE_BYTES && !cache.is_empty() {
                let mut oldest_key = None;
                let mut oldest_time = Instant::now();
                
                for (k, entry) in cache.iter() {
                    if entry.last_accessed < oldest_time {
                        oldest_time = entry.last_accessed;
                        oldest_key = Some(k.clone());
                    }
                }
                
                if let Some(k) = oldest_key {
                    if let Some(removed) = cache.remove(&k) {
                        total_size -= removed.content.len();
                        log_info(&format!("Evicted entry {} from cache due to size limit", k));
                    }
                } else {
                    break;
                }
            }
            
            cache.insert(
                ccr_id.clone(),
                CacheEntry {
                    content: req.0.raw_text.clone(),
                    last_accessed: Instant::now(),
                },
            );
        }

        Ok(format!(
            "{} \n\n[CCR Ref: {} - call retrieve_original tool to inspect full content if needed]",
            compressed, ccr_id
        ))
    }

    #[tool(
        description = "Retrieves the original, uncompressed raw text for a given CCR reference ID or a file path (starts with file:// or absolute path)."
    )]
    async fn retrieve_original(
        &self,
        req: Parameters<RetrieveOriginalRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        let input = req.0.ccr_id.trim();
        log_info(&format!("retrieve_original: {}", input));

        if input.starts_with("file://")
            || input.starts_with('/')
            || input.contains('/')
            || input.contains('\\')
        {
            // Sandboxed file read restricted to workspace
            let path_str = input.trim_start_matches("file://");
            let path = Path::new(path_str);
            let workspace_root = std::env::current_dir()
                .map_err(mcp_error)?
                .canonicalize()
                .map_err(mcp_error)?;

            let absolute_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                workspace_root.join(path)
            };

            let canonical = absolute_path.canonicalize().map_err(|e| {
                rmcp::ErrorData::internal_error(
                    format!("Path verification failed for '{}': {}", path_str, e),
                    None,
                )
            })?;

            if !canonical.starts_with(&workspace_root) {
                log_error(&format!("Path traversal blocked: {}", path_str));
                return Err(rmcp::ErrorData::internal_error(
                    format!("Access denied: path '{}' escapes workspace root", path_str),
                    None,
                ));
            }

            match fs::read_to_string(canonical) {
                Ok(content) => Ok(content),
                Err(e) => Err(rmcp::ErrorData::internal_error(
                    format!("Failed to read file from path '{}': {}", path_str, e),
                    None,
                )),
            }
        } else {
            // Retrieve from cache and update last_accessed time for LRU
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(entry) = cache.get_mut(input) {
                entry.last_accessed = Instant::now();
                Ok(entry.content.clone())
            } else {
                Err(rmcp::ErrorData::internal_error(
                    format!("CCR reference ID '{}' not found or expired.", input),
                    None,
                ))
            }
        }
    }

    #[tool(
        description = "Reads a file from the workspace, auto-detects its content type, compresses it, and registers a CCR reference ID."
    )]
    async fn compress_file(
        &self,
        req: Parameters<CompressFileRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        let path_str = req.0.file_path.trim_start_matches("file://");
        log_info(&format!("compress_file: {}", path_str));
        let path = Path::new(path_str);
        
        let workspace_root = std::env::current_dir()
            .map_err(mcp_error)?
            .canonicalize()
            .map_err(mcp_error)?;

        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        };
        
        let canonical = absolute_path.canonicalize().map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Path verification failed for '{}': {}", path_str, e),
                None,
            )
        })?;
        
        if !canonical.starts_with(&workspace_root) {
            log_error(&format!("Path traversal blocked: {}", path_str));
            return Err(rmcp::ErrorData::internal_error(
                format!("Access denied: path '{}' escapes workspace root", path_str),
                None,
            ));
        }

        let raw_text = fs::read_to_string(&canonical).map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Failed to read file '{}': {}", path_str, e),
                None,
            )
        })?;

        if raw_text.len() > MAX_INPUT_SIZE {
            return Err(rmcp::ErrorData::internal_error(
                format!("File size exceeds maximum allowed size of {} bytes", MAX_INPUT_SIZE),
                None,
            ));
        }

        let content_type = req.0.content_type.unwrap_or_else(|| "auto".to_string()).to_lowercase();
        let content_type_ref = if content_type == "auto" || content_type.is_empty() {
            detect_content_type_from_ext(&canonical)
                .unwrap_or_else(|| auto_detect_content_type(&raw_text))
        } else {
            match content_type.as_str() {
                "json" | "code" | "text_logs" | "csv" | "markdown" | "yaml" => content_type.as_str(),
                other => return Err(rmcp::ErrorData::internal_error(
                    format!("Unknown content_type '{}'. Use 'json', 'code', 'text_logs', 'csv', 'markdown', 'yaml', or 'auto'.", other),
                    None,
                )),
            }
        };

        let threshold = req.0.threshold;
        let compressed = match content_type_ref {
            "json" => compress_json(&raw_text, threshold.unwrap_or(DEFAULT_JSON_THRESHOLD)).map_err(mcp_error)?,
            "code" | "yaml" | "markdown" => compress_code(&raw_text),
            "csv" => compress_csv(&raw_text),
            _ => compress_logs(&raw_text, threshold.unwrap_or(DEFAULT_LOG_THRESHOLD)),
        };

        // Generate highly unique CCR ID using timestamp and atomic counter
        let time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let ccr_id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);

        // Cache original text with LRU eviction
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            let mut total_size: usize = cache.values().map(|entry| entry.content.len()).sum();
            
            while total_size + raw_text.len() > MAX_CACHE_BYTES && !cache.is_empty() {
                let mut oldest_key = None;
                let mut oldest_time = Instant::now();
                
                for (k, entry) in cache.iter() {
                    if entry.last_accessed < oldest_time {
                        oldest_time = entry.last_accessed;
                        oldest_key = Some(k.clone());
                    }
                }
                
                if let Some(k) = oldest_key {
                    if let Some(removed) = cache.remove(&k) {
                        total_size -= removed.content.len();
                        log_info(&format!("Evicted entry {} from cache due to size limit", k));
                    }
                } else {
                    break;
                }
            }
            
            cache.insert(
                ccr_id.clone(),
                CacheEntry {
                    content: raw_text,
                    last_accessed: Instant::now(),
                },
            );
        }

        Ok(format!(
            "{} \n\n[CCR Ref: {} - call retrieve_original tool to inspect full content if needed]",
            compressed, ccr_id
        ))
    }

    #[tool(description = "Returns statistics about the context cache.")]
    async fn cache_stats(
        &self,
        _req: Parameters<CacheStatsRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        log_info("cache_stats");
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let count = cache.len();
        let mut total_bytes = 0;
        let mut items = Vec::new();

        for (k, entry) in cache.iter() {
            let bytes = entry.content.len();
            total_bytes += bytes;
            items.push(format!("  - {}: {} bytes", k, bytes));
        }

        let items_str = if items.is_empty() {
            "No cached items.".to_string()
        } else {
            items.join("\n")
        };

        Ok(format!(
            "Cache Stats:\n- Total Items: {}\n- Total Size: {} bytes\n\nCached Entries:\n{}",
            count, total_bytes, items_str
        ))
    }

    #[tool(description = "Clears all cached context entries to free memory.")]
    async fn clear_cache(
        &self,
        _req: Parameters<ClearCacheRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        log_info("clear_cache");
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let count = cache.len();
        let total_bytes: usize = cache.values().map(|entry| entry.content.len()).sum();
        cache.clear();
        Ok(format!(
            "Successfully cleared cache. Evicted {} items (freed {} bytes).",
            count, total_bytes
        ))
    }

    #[tool(description = "Returns information about the Headroom MCP server configuration and status.")]
    async fn server_info(
        &self,
        _req: Parameters<ServerInfoRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        log_info("server_info");
        let uptime_secs = self.start_time.elapsed().as_secs();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let count = cache.len();
        
        Ok(format!(
            "Headroom MCP Server Info:\n\
             - Version: {}\n\
             - Uptime: {}s\n\
             - Cache Size: {} items\n\
             - Default Log Threshold: {} chars\n\
             - Default JSON Threshold: {} chars\n\
             - Max Input Size: {} bytes",
            env!("CARGO_PKG_VERSION"),
            uptime_secs,
            count,
            DEFAULT_LOG_THRESHOLD,
            DEFAULT_JSON_THRESHOLD,
            MAX_INPUT_SIZE
        ))
    }
}

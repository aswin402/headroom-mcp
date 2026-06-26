use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use crate::config::{Config, SCOPE_FILES};
use crate::intelligence::tokens::estimate_tokens;
use crate::compression::{
    auto_detect_content_type, compress_code, compress_csv, compress_json, compress_logs,
    detect_content_type_from_ext,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct HeadroomServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    pub config: Arc<Config>,
    pub cache: Arc<dyn crate::cache::CacheBackend>,
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
    #[schemars(description = "If true, returns a preview of compression without caching.")]
    pub preview: Option<bool>,
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
    #[schemars(description = "If true, returns a preview of compression without caching.")]
    pub preview: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CacheStatsRequest {}

#[derive(Deserialize, JsonSchema)]
pub struct ClearCacheRequest {}

#[derive(Deserialize, JsonSchema)]
pub struct ServerInfoRequest {}

#[derive(Deserialize, JsonSchema)]
pub struct PingRequest {}

#[derive(Deserialize, JsonSchema)]
pub struct CountTokensRequest {
    #[schemars(description = "The text to estimate tokens for.")]
    pub text: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchCacheRequest {
    #[schemars(description = "Search query to find relevant cached entries.")]
    pub query: String,
    #[schemars(description = "Maximum number of results to return.")]
    pub max_results: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExportCacheRequest {
    #[schemars(description = "File path to export cache to (JSON format).")]
    pub file_path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImportCacheRequest {
    #[schemars(description = "File path to import cache from (JSON format).")]
    pub file_path: String,
}

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
    pub fn new(config: Arc<Config>, cache: Arc<dyn crate::cache::CacheBackend>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            config,
            cache,
            start_time: Instant::now(),
        }
    }

    fn get_workspace_root(&self) -> Result<std::path::PathBuf, rmcp::ErrorData> {
        if let Some(ref root_str) = self.config.workspace_root {
            Path::new(root_str)
                .canonicalize()
                .map_err(|e| rmcp::ErrorData::internal_error(
                    format!("Failed to resolve configured workspace root '{}': {}", root_str, e),
                    None,
                ))
        } else {
            std::env::current_dir()
                .map_err(mcp_error)?
                .canonicalize()
                .map_err(mcp_error)
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
        
        let workspace_root = self.get_workspace_root()?;

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
        if raw_text.len() > self.config.max_input_size {
            return Err(rmcp::ErrorData::internal_error(
                format!("Content size exceeds maximum allowed size of {} bytes", self.config.max_input_size),
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
            "json" => compress_json(raw_text, threshold.unwrap_or(self.config.json_threshold)).map_err(mcp_error)?,
            "code" | "yaml" | "markdown" => compress_code(raw_text),
            "csv" => compress_csv(raw_text),
            _ => compress_logs(raw_text, threshold.unwrap_or(self.config.log_threshold)),
        };

        let is_preview = req.0.preview.unwrap_or(false);
        let ccr_id = if is_preview {
            "PREVIEW".to_string()
        } else {
            // Generate highly unique CCR ID using timestamp and atomic counter
            let time_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);
            self.cache.insert(&id, req.0.raw_text.trim(), None).map_err(mcp_error)?;
            id
        };

        let original_tokens = estimate_tokens(raw_text);
        let compressed_tokens = estimate_tokens(&compressed);
        let saved_pct = if original_tokens > 0 {
            let saved = (original_tokens as f64 - compressed_tokens as f64) / original_tokens as f64 * 100.0;
            format!("{:.1}%", saved.max(0.0))
        } else {
            "0.0%".to_string()
        };

        if is_preview {
            Ok(format!(
                "{}\n\n[PREVIEW - not cached | Original: ~{} tokens | Compressed: ~{} tokens | Saved: {} | call again with preview=false to register CCR]",
                compressed, original_tokens, compressed_tokens, saved_pct
            ))
        } else {
            Ok(format!(
                "{}\n\n[CCR Ref: {} | Original: ~{} tokens | Compressed: ~{} tokens | Saved: {} | call retrieve_original tool to inspect full content if needed]",
                compressed, ccr_id, original_tokens, compressed_tokens, saved_pct
            ))
        }
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
            let workspace_root = self.get_workspace_root()?;

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
            // Retrieve from cache and update last_accessed time
            match self.cache.get(input).map_err(mcp_error)? {
                Some(content) => Ok(content),
                None => Err(rmcp::ErrorData::internal_error(
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
        
        let workspace_root = self.get_workspace_root()?;

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

        if raw_text.len() > self.config.max_input_size {
            return Err(rmcp::ErrorData::internal_error(
                format!("File size exceeds maximum allowed size of {} bytes", self.config.max_input_size),
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
            "json" => compress_json(&raw_text, threshold.unwrap_or(self.config.json_threshold)).map_err(mcp_error)?,
            "code" | "yaml" | "markdown" => compress_code(&raw_text),
            "csv" => compress_csv(&raw_text),
            _ => compress_logs(&raw_text, threshold.unwrap_or(self.config.log_threshold)),
        };

        let is_preview = req.0.preview.unwrap_or(false);
        let ccr_id = if is_preview {
            "PREVIEW".to_string()
        } else {
            // Generate highly unique CCR ID using timestamp and atomic counter
            let time_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);
            self.cache.insert(&id, &raw_text, None).map_err(mcp_error)?;
            id
        };

        let original_tokens = estimate_tokens(&raw_text);
        let compressed_tokens = estimate_tokens(&compressed);
        let saved_pct = if original_tokens > 0 {
            let saved = (original_tokens as f64 - compressed_tokens as f64) / original_tokens as f64 * 100.0;
            format!("{:.1}%", saved.max(0.0))
        } else {
            "0.0%".to_string()
        };

        if is_preview {
            Ok(format!(
                "{}\n\n[PREVIEW - not cached | Original: ~{} tokens | Compressed: ~{} tokens | Saved: {} | call again with preview=false to register CCR]",
                compressed, original_tokens, compressed_tokens, saved_pct
            ))
        } else {
            Ok(format!(
                "{}\n\n[CCR Ref: {} | Original: ~{} tokens | Compressed: ~{} tokens | Saved: {} | call retrieve_original tool to inspect full content if needed]",
                compressed, ccr_id, original_tokens, compressed_tokens, saved_pct
            ))
        }
    }

    #[tool(description = "Returns statistics about the context cache.")]
    async fn cache_stats(
        &self,
        _req: Parameters<CacheStatsRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        log_info("cache_stats");
        let count = self.cache.len().map_err(mcp_error)?;
        let total_bytes = self.cache.total_bytes().map_err(mcp_error)?;
        let stats = self.cache.stats().map_err(mcp_error)?;
        let mut items = Vec::new();

        for (k, bytes) in stats {
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
        let (count, total_bytes) = self.cache.clear().map_err(mcp_error)?;
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
        let count = self.cache.len().map_err(mcp_error)?;
        
        Ok(format!(
            "Headroom MCP Server Info:\n\
             - Version: {}\n\
             - Uptime: {}s\n\
             - Cache Size: {} items\n\
             - Default Log Threshold: {} chars\n\
             - Default JSON Threshold: {} chars\n\
             - Max Input Size: {} bytes\n\
             - Max Cache Size: {} bytes\n\
             - Workspace Root: {:?}",
            env!("CARGO_PKG_VERSION"),
            uptime_secs,
            count,
            self.config.log_threshold,
            self.config.json_threshold,
            self.config.max_input_size,
            self.config.max_cache_bytes,
            self.config.workspace_root
        ))
    }

    #[tool(description = "Health check. Returns 'ok' if the server is responsive.")]
    async fn ping(&self, _req: Parameters<PingRequest>) -> Result<String, rmcp::ErrorData> {
        log_info("ping");
        Ok("ok".to_string())
    }

    #[tool(description = "Estimates the token count for a given text. Helps agents decide whether compression is needed.")]
    async fn count_tokens(&self, req: Parameters<CountTokensRequest>) -> Result<String, rmcp::ErrorData> {
        log_info("count_tokens");
        let tokens = estimate_tokens(&req.0.text);
        let chars = req.0.text.chars().count();
        Ok(format!("Token estimate: {} tokens ({} characters)", tokens, chars))
    }

    #[tool(description = "Searches cached content by keyword. Returns matching CCR IDs and content snippets.")]
    async fn search_cache(&self, req: Parameters<SearchCacheRequest>) -> Result<String, rmcp::ErrorData> {
        log_info(&format!("search_cache: {}", req.0.query));
        let results = self.cache.search(&req.0.query).map_err(mcp_error)?;
        let max = req.0.max_results.unwrap_or(10);
        let items: Vec<String> = results.iter().take(max).map(|(id, snippet)| {
            format!("- {}: {}", id, snippet)
        }).collect();

        if items.is_empty() {
            Ok(format!("No matches found for query '{}'.", req.0.query))
        } else {
            Ok(format!("Found {} matches:\n{}", items.len(), items.join("\n")))
        }
    }

    #[tool(description = "Exports the entire cache to a JSON file for session portability.")]
    async fn export_cache(&self, req: Parameters<ExportCacheRequest>) -> Result<String, rmcp::ErrorData> {
        let path_str = req.0.file_path.trim_start_matches("file://");
        log_info(&format!("export_cache to: {}", path_str));
        
        let workspace_root = self.get_workspace_root()?;
        let path = Path::new(path_str);
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        };

        let parent = absolute_path.parent().unwrap_or(&workspace_root);
        let canonical_parent = parent.canonicalize().map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Parent directory verification failed for '{}': {}", path_str, e),
                None,
            )
        })?;

        if !canonical_parent.starts_with(&workspace_root) {
            log_error(&format!("Access denied for export path: {}", path_str));
            return Err(rmcp::ErrorData::internal_error(
                format!("Access denied: export path '{}' escapes workspace root", path_str),
                None,
            ));
        }

        let entries = self.cache.export_all().map_err(mcp_error)?;
        let json_str = serde_json::to_string_pretty(&entries).map_err(mcp_error)?;
        
        fs::write(&absolute_path, json_str).map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Failed to write export file: {}", e),
                None,
            )
        })?;

        Ok(format!("Successfully exported {} cache entries to '{}'.", entries.len(), path_str))
    }

    #[tool(description = "Imports cached entries from a previously exported JSON file.")]
    async fn import_cache(&self, req: Parameters<ImportCacheRequest>) -> Result<String, rmcp::ErrorData> {
        let path_str = req.0.file_path.trim_start_matches("file://");
        log_info(&format!("import_cache from: {}", path_str));

        let workspace_root = self.get_workspace_root()?;
        let path = Path::new(path_str);
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
            log_error(&format!("Access denied for import path: {}", path_str));
            return Err(rmcp::ErrorData::internal_error(
                format!("Access denied: import path '{}' escapes workspace root", path_str),
                None,
            ));
        }

        let json_str = fs::read_to_string(canonical).map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("Failed to read import file: {}", e),
                None,
            )
        })?;

        let entries: Vec<(String, String, Option<String>, String)> = serde_json::from_str(&json_str).map_err(mcp_error)?;
        let mut count = 0;
        for (id, content, session, _) in &entries {
            self.cache.insert(id, content, session.as_deref()).map_err(mcp_error)?;
            count += 1;
        }

        Ok(format!("Successfully imported {} cache entries from '{}'.", count, path_str))
    }
}

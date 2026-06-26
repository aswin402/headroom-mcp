use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use crate::config::{Config, SCOPE_FILES};
use crate::intelligence::tokens::estimate_tokens;
use crate::compression::{
    auto_detect_content_type, compress_csv, compress_json, compress_logs,
    detect_content_type_from_ext,
};
use crate::compression::diff::compress_diff;

pub(crate) static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct HeadroomServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    pub config: Arc<Config>,
    pub cache: Arc<dyn crate::cache::CacheBackend>,
    pub metrics: Arc<crate::metrics::Metrics>,
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
    #[schemars(description = "If true, extracts only structural code signatures (functions/classes) and discards bodies.")]
    pub signatures_only: Option<bool>,
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
    #[schemars(description = "If true, extracts only structural code signatures (functions/classes) and discards bodies.")]
    pub signatures_only: Option<bool>,
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

#[derive(Deserialize, JsonSchema)]
pub struct CompressDiffRequest {
    #[schemars(description = "Raw unified diff output to compress.")]
    pub diff_text: String,
    #[schemars(description = "If true, returns a preview of compression without caching.")]
    pub preview: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompressDirectoryRequest {
    #[schemars(description = "Path to the directory to compress.")]
    pub dir_path: String,
    #[schemars(description = "File extensions to include (e.g. ['rs', 'py']). Empty = all.")]
    pub extensions: Option<Vec<String>>,
    #[schemars(description = "Maximum depth to recurse (0 = unlimited).")]
    pub max_depth: Option<usize>,
    #[schemars(description = "If true, returns a preview of compression without caching.")]
    pub preview: Option<bool>,
    #[schemars(description = "If true, extracts only structural code signatures (functions/classes) and discards bodies.")]
    pub signatures_only: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SummarizeCodebaseRequest {
    #[schemars(description = "Root path of the codebase (default: workspace root).")]
    pub root_path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompressUrlRequest {
    #[schemars(description = "The HTTP/HTTPS URL to fetch and compress.")]
    pub url: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RunAndCompressRequest {
    #[schemars(description = "The shell command to run.")]
    pub command: String,
    #[schemars(description = "Optional arguments to pass to the command.")]
    pub args: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CacheAlignRequest {
    #[schemars(description = "The text chunks to align.")]
    pub chunks: Vec<String>,
    #[schemars(description = "Optional custom padding size (default: 1024 characters).")]
    pub padding_size: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompressSchemaRequest {
    #[schemars(description = "The JSON schema to minify.")]
    pub schema: String,
}

pub(crate) fn mcp_error<E: std::fmt::Display>(err: E) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(err.to_string(), None)
}

pub(crate) fn log_info(msg: &str) {
    eprintln!("[Headroom MCP] [INFO] {}", msg);
}

pub(crate) fn log_error(msg: &str) {
    eprintln!("[Headroom MCP] [ERROR] {}", msg);
}

// --- Helper Structs for Directory & Codebase Tools ---

struct TreeNode {
    name: String,
    is_dir: bool,
    line_count: usize,
    files_count: usize,
    children: BTreeMap<String, TreeNode>,
}

impl TreeNode {
    fn insert(&mut self, parts: &[String], is_file: bool, line_count: usize) {
        if parts.is_empty() {
            return;
        }
        let name = &parts[0];
        if parts.len() == 1 {
            let child = TreeNode {
                name: name.clone(),
                is_dir: !is_file,
                line_count,
                files_count: if is_file { 1 } else { 0 },
                children: BTreeMap::new(),
            };
            self.children.insert(name.clone(), child);
        } else {
            let entry = self.children.entry(name.clone()).or_insert_with(|| TreeNode {
                name: name.clone(),
                is_dir: true,
                line_count: 0,
                files_count: 0,
                children: BTreeMap::new(),
            });
            entry.insert(&parts[1..], is_file, line_count);
        }
    }

    fn update_counts(&mut self) -> (usize, usize) {
        if !self.is_dir {
            return (self.files_count, self.line_count);
        }
        let mut total_files = 0;
        let mut total_lines = 0;
        for child in self.children.values_mut() {
            let (f, l) = child.update_counts();
            total_files += f;
            total_lines += l;
        }
        self.files_count = total_files;
        self.line_count = total_lines;
        (total_files, total_lines)
    }

    fn format_tree(&self, prefix: &str, is_last: bool, depth: usize, max_depth: usize) -> String {
        if depth > max_depth {
            return "".to_string();
        }
        let mut result = String::new();
        if depth > 0 {
            let connector = if is_last { "└── " } else { "├── " };
            result.push_str(prefix);
            result.push_str(connector);
            if self.is_dir {
                result.push_str(&format!("{}/ ({} file{}, {} lines)\n", self.name, self.files_count, if self.files_count == 1 { "" } else { "s" }, self.line_count));
            } else {
                result.push_str(&format!("{} ({} lines)\n", self.name, self.line_count));
            }
        }

        if self.is_dir && depth < max_depth {
            let new_prefix = if depth == 0 {
                "".to_string()
            } else {
                format!("{}{}", prefix, if is_last { "    " } else { "│   " })
            };
            let children_vec: Vec<&TreeNode> = self.children.values().collect();
            for (i, child) in children_vec.iter().enumerate() {
                let last_child = i == children_vec.len() - 1;
                result.push_str(&child.format_tree(&new_prefix, last_child, depth + 1, max_depth));
            }
        }
        result
    }
}

struct CompTreeFile {
    ccr_id: String,
    original_tokens: usize,
    compressed_tokens: usize,
    saved_pct: String,
}

struct CompTreeNode {
    name: String,
    is_dir: bool,
    files_count: usize,
    file_info: Option<CompTreeFile>,
    children: BTreeMap<String, CompTreeNode>,
}

impl CompTreeNode {
    fn insert(&mut self, parts: &[String], file_info: CompTreeFile) {
        if parts.is_empty() {
            return;
        }
        let name = &parts[0];
        if parts.len() == 1 {
            let child = CompTreeNode {
                name: name.clone(),
                is_dir: false,
                files_count: 1,
                file_info: Some(file_info),
                children: BTreeMap::new(),
            };
            self.children.insert(name.clone(), child);
        } else {
            let entry = self.children.entry(name.clone()).or_insert_with(|| CompTreeNode {
                name: name.clone(),
                is_dir: true,
                files_count: 0,
                file_info: None,
                children: BTreeMap::new(),
            });
            entry.insert(&parts[1..], file_info);
        }
    }

    fn update_counts(&mut self) -> usize {
        if !self.is_dir {
            return 1;
        }
        let mut total = 0;
        for child in self.children.values_mut() {
            total += child.update_counts();
        }
        self.files_count = total;
        total
    }

    fn format_tree(&self, prefix: &str, is_last: bool, depth: usize, max_depth: usize) -> String {
        if depth > max_depth {
            return "".to_string();
        }
        let mut result = String::new();
        if depth > 0 {
            let connector = if is_last { "└── " } else { "├── " };
            result.push_str(prefix);
            result.push_str(connector);
            if self.is_dir {
                result.push_str(&format!("{}/ ({} file{} compressed)\n", self.name, self.files_count, if self.files_count == 1 { "" } else { "s" }));
            } else if let Some(ref info) = self.file_info {
                result.push_str(&format!(
                    "{} [CCR: {} | ~{} -> ~{} tokens | saved {}]\n",
                    self.name, info.ccr_id, info.original_tokens, info.compressed_tokens, info.saved_pct
                ));
            } else {
                result.push_str(&format!("{}\n", self.name));
            }
        }

        if self.is_dir && depth < max_depth {
            let new_prefix = if depth == 0 {
                "".to_string()
            } else {
                format!("{}{}", prefix, if is_last { "    " } else { "│   " })
            };
            let children_vec: Vec<&CompTreeNode> = self.children.values().collect();
            for (i, child) in children_vec.iter().enumerate() {
                let last_child = i == children_vec.len() - 1;
                result.push_str(&child.format_tree(&new_prefix, last_child, depth + 1, max_depth));
            }
        }
        result
    }
}

fn is_binary_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" |
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" |
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" |
            "exe" | "dll" | "so" | "dylib" | "bin" | "node" |
            "mp3" | "mp4" | "wav" | "avi" | "mkv" | "mov" => return true,
            _ => {}
        }
    }
    if let Ok(mut file) = fs::File::open(path) {
        use std::io::Read;
        let mut buffer = [0; 1024];
        if let Ok(bytes_read) = file.read(&mut buffer) {
            if buffer[..bytes_read].contains(&0) {
                return true;
            }
        }
    }
    false
}

fn detect_project_type(root: &Path) -> String {
    if root.join("Cargo.toml").exists() {
        "Rust".to_string()
    } else if root.join("package.json").exists() {
        "Node.js".to_string()
    } else if root.join("go.mod").exists() {
        "Go".to_string()
    } else if root.join("requirements.txt").exists()
        || root.join("pyproject.toml").exists()
        || root.join("Pipfile").exists()
    {
        "Python".to_string()
    } else if root.join("pom.xml").exists() || root.join("build.gradle").exists() {
        "Java".to_string()
    } else if root.join("CMakeLists.txt").exists() {
        "C/C++".to_string()
    } else {
        "Generic/Unknown".to_string()
    }
}

fn minify_json_map(map: &mut serde_json::Map<String, serde_json::Value>) {
    map.remove("description");
    map.remove("title");
    map.remove("examples");
    for (_, v) in map.iter_mut() {
        minify_json_value(v);
    }
}

fn minify_json_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            minify_json_map(map);
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                minify_json_value(item);
            }
        }
        _ => {}
    }
}

#[tool_router(server_handler)]
impl HeadroomServer {
    pub fn new(
        config: Arc<Config>,
        cache: Arc<dyn crate::cache::CacheBackend>,
        metrics: Arc<crate::metrics::Metrics>,
    ) -> Self {
        let mut tool_router = Self::tool_router();
        if config.compact_schemas {
            log_info("Compacting registered tool schemas to save token budget");
            for (_, route) in tool_router.map.iter_mut() {
                let input_map = Arc::make_mut(&mut route.attr.input_schema);
                minify_json_map(input_map);
            }
        }
        Self {
            tool_router,
            config,
            cache,
            metrics,
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

    fn resolve_and_verify_path(&self, input_path: &str) -> Result<std::path::PathBuf, rmcp::ErrorData> {
        let workspace_root = self.get_workspace_root()?;
        let path = Path::new(input_path.trim_start_matches("file://"));
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        };

        let resolved_path = match absolute_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return Err(rmcp::ErrorData::internal_error(
                    format!("Path '{}' does not exist or cannot be accessed: {}", input_path, e),
                    None,
                ));
            }
        };

        if !resolved_path.starts_with(&workspace_root) {
            log_error(&format!("Access denied for path: {}", input_path));
            return Err(rmcp::ErrorData::internal_error(
                format!("Access denied: path '{}' is outside workspace root", input_path),
                None,
            ));
        }

        Ok(resolved_path)
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
            "code" | "yaml" | "markdown" => {
                let signatures_only = req.0.signatures_only.unwrap_or(false);
                crate::compression::code::compress_code_with_options(raw_text, signatures_only, "")
            }
            "csv" => compress_csv(raw_text),
            _ => compress_logs(raw_text, threshold.unwrap_or(self.config.log_threshold)),
        };

        self.metrics.compressions_total.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_bytes_compressed.fetch_add(raw_text.len() as u64, Ordering::Relaxed);
        self.metrics.total_bytes_saved.fetch_add(raw_text.len().saturating_sub(compressed.len()) as u64, Ordering::Relaxed);

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

        self.metrics.retrievals_total.fetch_add(1, Ordering::Relaxed);

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
                Some(content) => {
                    self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
                    Ok(content)
                }
                None => {
                    self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
                    Err(rmcp::ErrorData::internal_error(
                        format!("CCR reference ID '{}' not found or expired.", input),
                        None,
                    ))
                }
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
        let ext = canonical.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let compressed = match content_type_ref {
            "json" => compress_json(&raw_text, threshold.unwrap_or(self.config.json_threshold)).map_err(mcp_error)?,
            "code" | "yaml" | "markdown" => {
                let signatures_only = req.0.signatures_only.unwrap_or(false);
                crate::compression::code::compress_code_with_options(&raw_text, signatures_only, ext)
            }
            "csv" => compress_csv(&raw_text),
            _ => compress_logs(&raw_text, threshold.unwrap_or(self.config.log_threshold)),
        };

        self.metrics.compressions_total.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_bytes_compressed.fetch_add(raw_text.len() as u64, Ordering::Relaxed);
        self.metrics.total_bytes_saved.fetch_add(raw_text.len().saturating_sub(compressed.len()) as u64, Ordering::Relaxed);

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
        
        let metrics_json = self.metrics.to_json();
        Ok(format!(
            "Headroom MCP Server Info:\n\
             - Version: {}\n\
             - Uptime: {}s\n\
             - Cache Size: {} items\n\
             - Default Log Threshold: {} chars\n\
             - Default JSON Threshold: {} chars\n\
             - Max Input Size: {} bytes\n\
             - Max Cache Size: {} bytes\n\
             - Workspace Root: {:?}\n\
             - Metrics: {}",
            env!("CARGO_PKG_VERSION"),
            uptime_secs,
            count,
            self.config.log_threshold,
            self.config.json_threshold,
            self.config.max_input_size,
            self.config.max_cache_bytes,
            self.config.workspace_root,
            metrics_json
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

    #[tool(
        description = "Compresses unified diff output into a structural summary and caches the full diff."
    )]
    async fn compress_diff(
        &self,
        req: Parameters<CompressDiffRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        let diff_text = req.0.diff_text.trim();
        if diff_text.is_empty() {
            return Ok("Empty diff provided.".to_string());
        }

        // Max input size check
        if diff_text.len() > self.config.max_input_size {
            return Err(rmcp::ErrorData::internal_error(
                format!("Diff size exceeds maximum allowed size of {} bytes", self.config.max_input_size),
                None,
            ));
        }

        log_info("compress_diff");

        let compressed = compress_diff(diff_text);
        self.metrics.compressions_total.fetch_add(1, Ordering::Relaxed);
        self.metrics.total_bytes_compressed.fetch_add(diff_text.len() as u64, Ordering::Relaxed);
        self.metrics.total_bytes_saved.fetch_add(diff_text.len().saturating_sub(compressed.len()) as u64, Ordering::Relaxed);
        let is_preview = req.0.preview.unwrap_or(false);

        let ccr_id = if is_preview {
            "PREVIEW".to_string()
        } else {
            let time_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);
            self.cache.insert(&id, diff_text, None).map_err(mcp_error)?;
            id
        };

        let original_tokens = estimate_tokens(diff_text);
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
        description = "Recursively walks a directory, compresses each matching file, and registers CCR reference tokens for the agent."
    )]
    async fn compress_directory(
        &self,
        req: Parameters<CompressDirectoryRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        let dir_path_str = &req.0.dir_path;
        log_info(&format!("compress_directory: {}", dir_path_str));

        let resolved_dir = self.resolve_and_verify_path(dir_path_str)?;

        let mut walk_builder = ignore::WalkBuilder::new(&resolved_dir);
        walk_builder.git_ignore(true);
        walk_builder.hidden(false); // include hidden, but skip .git component manually
        
        if let Some(depth) = req.0.max_depth {
            if depth > 0 {
                walk_builder.max_depth(Some(depth));
            }
        }

        let walker = walk_builder.build();
        let mut processed_files = 0;
        let file_limit = 500;
        let mut tree_root = CompTreeNode {
            name: resolved_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(".")
                .to_string(),
            is_dir: true,
            files_count: 0,
            file_info: None,
            children: BTreeMap::new(),
        };

        let extensions = req.0.extensions.clone().unwrap_or_default();
        let is_preview = req.0.preview.unwrap_or(false);

        for result in walker {
            let entry = match result {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_dir() {
                continue;
            }

            if path.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }

            if !extensions.is_empty() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !extensions.iter().any(|filter| filter.to_lowercase() == ext) {
                    continue;
                }
            }

            if is_binary_file(path) {
                continue;
            }

            let raw_text = match fs::read_to_string(path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            if raw_text.len() > self.config.max_input_size {
                continue;
            }

            let content_type_ref = detect_content_type_from_ext(path)
                .unwrap_or_else(|| auto_detect_content_type(&raw_text));

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let compressed = match content_type_ref {
                "json" => match compress_json(&raw_text, self.config.json_threshold) {
                    Ok(c) => c,
                    Err(_) => continue,
                },
                "code" | "yaml" | "markdown" => {
                    let signatures_only = req.0.signatures_only.unwrap_or(false);
                    crate::compression::code::compress_code_with_options(&raw_text, signatures_only, ext)
                }
                "csv" => compress_csv(&raw_text),
                _ => compress_logs(&raw_text, self.config.log_threshold),
            };

            self.metrics.compressions_total.fetch_add(1, Ordering::Relaxed);
            self.metrics.total_bytes_compressed.fetch_add(raw_text.len() as u64, Ordering::Relaxed);
            self.metrics.total_bytes_saved.fetch_add(raw_text.len().saturating_sub(compressed.len()) as u64, Ordering::Relaxed);

            let ccr_id = if is_preview {
                "PREVIEW".to_string()
            } else {
                let time_ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
                let id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);
                self.cache
                    .insert(&id, raw_text.trim(), None)
                    .map_err(mcp_error)?;
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

            if let Ok(rel_path) = path.strip_prefix(&resolved_dir) {
                let parts: Vec<String> = rel_path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();

                let file_info = CompTreeFile {
                    ccr_id,
                    original_tokens,
                    compressed_tokens,
                    saved_pct,
                };

                tree_root.insert(&parts, file_info);
            }

            processed_files += 1;
            if processed_files >= file_limit {
                break;
            }
        }

        tree_root.update_counts();
        let max_depth = req.0.max_depth.unwrap_or(4);
        let tree_str = tree_root.format_tree("", true, 0, max_depth);

        let preview_label = if is_preview {
            " [PREVIEW - not cached]"
        } else {
            ""
        };

        let suffix = if processed_files >= file_limit {
            format!("\nWarning: Walk stopped early because file count limit ({}) was reached.", file_limit)
        } else {
            "".to_string()
        };

        Ok(format!(
            "Compressed directory: {} ({} files processed){}\n\n{}{}",
            dir_path_str, processed_files, preview_label, tree_str, suffix
        ))
    }

    #[tool(
        description = "Analyzes the codebase and returns a summary of language usage, file sizes, and directory layout."
    )]
    async fn summarize_codebase(
        &self,
        req: Parameters<SummarizeCodebaseRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        let root_path_str = req.0.root_path.as_deref().unwrap_or(".");
        log_info(&format!("summarize_codebase: {}", root_path_str));

        let resolved_root = self.resolve_and_verify_path(root_path_str)?;

        let mut walk_builder = ignore::WalkBuilder::new(&resolved_root);
        walk_builder.git_ignore(true);
        walk_builder.hidden(false);

        let walker = walk_builder.build();
        let mut total_files = 0;
        let mut total_lines = 0;
        let mut ext_counts: HashMap<String, (usize, usize)> = HashMap::new();

        let mut tree_root = TreeNode {
            name: resolved_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(".")
                .to_string(),
            is_dir: true,
            line_count: 0,
            files_count: 0,
            children: BTreeMap::new(),
        };

        let file_limit = 1000;

        for result in walker {
            let entry = match result {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_dir() {
                continue;
            }

            if path.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }

            if is_binary_file(path) {
                continue;
            }

            let file_lines = if let Ok(content) = fs::read_to_string(path) {
                content.lines().count()
            } else {
                continue;
            };

            total_files += 1;
            total_lines += file_lines;

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("no_ext")
                .to_lowercase();
            let entry_stats = ext_counts.entry(ext).or_insert((0, 0));
            entry_stats.0 += 1;
            entry_stats.1 += file_lines;

            if let Ok(rel_path) = path.strip_prefix(&resolved_root) {
                let parts: Vec<String> = rel_path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                tree_root.insert(&parts, true, file_lines);
            }

            if total_files >= file_limit {
                break;
            }
        }

        tree_root.update_counts();
        let tree_str = tree_root.format_tree("", true, 0, 3);
        let project_type = detect_project_type(&resolved_root);

        let mut breakdown_lines = Vec::new();
        let mut ext_vec: Vec<(&String, &(usize, usize))> = ext_counts.iter().collect();
        ext_vec.sort_by(|a, b| b.1.1.cmp(&a.1.1));

        for (ext, (count, lines)) in ext_vec {
            breakdown_lines.push(format!("- {}: {} file{}, {} lines", ext, count, if *count == 1 { "" } else { "s" }, lines));
        }

        let suffix = if total_files >= file_limit {
            format!("\nWarning: Walk stopped early because file limit ({}) was reached.", file_limit)
        } else {
            "".to_string()
        };

        Ok(format!(
            "Project: {} ({} project)\nTotal: {} files, {} lines of code\n\nBreakdown by extension:\n{}\n\nDirectory structure:\n{}{}",
            tree_root.name,
            project_type,
            total_files,
            total_lines,
            breakdown_lines.join("\n"),
            tree_str,
            suffix
        ))
    }

    #[tool(
        description = "Fetches a URL, extracts its text content, and returns a compressed summary with a CCR reference."
    )]
    async fn compress_url(
        &self,
        req: Parameters<CompressUrlRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        crate::tools::web::compress_url(self, req.0.url).await
    }

    #[tool(
        description = "Executes a shell command in the sandboxed workspace root and returns its compressed output with a CCR reference."
    )]
    async fn run_and_compress(
        &self,
        req: Parameters<RunAndCompressRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        crate::tools::exec::run_and_compress(self, req.0.command, req.0.args).await
    }

    #[tool(
        description = "Aligns context chunks deterministically, padding and wrapping them to optimize KV cache hits for LLM providers."
    )]
    async fn cache_align(
        &self,
        req: Parameters<CacheAlignRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        crate::tools::cache_align::cache_align(self, req.0.chunks, req.0.padding_size).await
    }

    #[tool(
        description = "Minifies a JSON schema representation of MCP tools, stripping descriptions and comments to save token budget."
    )]
    async fn compress_schema(
        &self,
        req: Parameters<CompressSchemaRequest>,
    ) -> Result<String, rmcp::ErrorData> {
        crate::tools::schema::compress_schema(self, req.0.schema).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::memory::MemoryCache;

    #[test]
    fn test_is_binary_file() {
        let temp_dir = std::env::temp_dir().join("headroom_test_binary");
        fs::create_dir_all(&temp_dir).unwrap();
        let txt_path = temp_dir.join("test.txt");
        let bin_path = temp_dir.join("test.bin");
        
        fs::write(&txt_path, "Hello world").unwrap();
        fs::write(&bin_path, b"Hello \x00 world").unwrap();

        assert!(!is_binary_file(&txt_path));
        assert!(is_binary_file(&bin_path));

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_detect_project_type() {
        let temp_dir = std::env::temp_dir().join("headroom_test_project");
        fs::create_dir_all(&temp_dir).unwrap();

        fs::write(temp_dir.join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_project_type(&temp_dir), "Rust");

        fs::remove_file(temp_dir.join("Cargo.toml")).unwrap();
        fs::write(temp_dir.join("package.json"), "").unwrap();
        assert_eq!(detect_project_type(&temp_dir), "Node.js");

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_summarize_codebase_and_compress_directory() {
        let temp_dir = std::env::temp_dir().join("headroom_test_suite");
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(temp_dir.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        fs::write(src_dir.join("main.rs"), "fn main() {\n    // comment\n    println!(\"hello\");\n}").unwrap();
        fs::write(src_dir.join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }").unwrap();

        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: Some(temp_dir.to_string_lossy().into_owned()),
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        // Test summarize_codebase
        let req = Parameters(SummarizeCodebaseRequest { root_path: None });
        let summary = server.summarize_codebase(req).await.unwrap();
        assert!(summary.contains("Project: headroom_test_suite (Rust project)"));
        assert!(summary.contains("Total: 3 files"));
        assert!(summary.contains("main.rs"));

        // Test compress_directory
        let req_comp = Parameters(CompressDirectoryRequest {
            dir_path: "src".to_string(),
            extensions: None,
            max_depth: None,
            preview: Some(false),
            signatures_only: None,
        });
        let comp_res = server.compress_directory(req_comp).await.unwrap();
        assert!(comp_res.contains("Compressed directory"));
        assert!(comp_res.contains("main.rs"));
        assert!(comp_res.contains("lib.rs"));

        // Test sandbox rejection
        let req_err = Parameters(SummarizeCodebaseRequest { root_path: Some("/etc".to_string()) });
        let summary_err = server.summarize_codebase(req_err).await;
        assert!(summary_err.is_err());

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_metrics_tracking() {
        let temp_dir = std::env::temp_dir().join("headroom_test_metrics");
        fs::create_dir_all(&temp_dir).unwrap();

        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: Some(temp_dir.to_string_lossy().into_owned()),
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let metrics = Arc::new(crate::metrics::Metrics::new());
        let server = HeadroomServer::new(config, cache, metrics.clone());

        // Initial state
        assert_eq!(metrics.compressions_total.load(Ordering::Relaxed), 0);

        // Compress content
        let req = Parameters(CompressContentRequest {
            raw_text: "let x = 1;\n// comment\nlet y = 2;".to_string(),
            content_type: "code".to_string(),
            threshold: None,
            preview: Some(false),
            signatures_only: None,
        });
        let compressed = server.compress_content(req).await.unwrap();
        assert!(compressed.contains("ccr_"));

        assert_eq!(metrics.compressions_total.load(Ordering::Relaxed), 1);
        assert!(metrics.total_bytes_compressed.load(Ordering::Relaxed) > 0);

        // Retrieve original (hit)
        let req_id = compressed.split("CCR Ref: ").nth(1).unwrap().split(" |").next().unwrap().trim();
        let req_retrieve = Parameters(RetrieveOriginalRequest { ccr_id: req_id.to_string() });
        let retrieved = server.retrieve_original(req_retrieve).await.unwrap();
        assert_eq!(retrieved, "let x = 1;\n// comment\nlet y = 2;");

        assert_eq!(metrics.retrievals_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.cache_hits.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.cache_misses.load(Ordering::Relaxed), 0);

        // Retrieve original (miss)
        let req_miss = Parameters(RetrieveOriginalRequest { ccr_id: "ccr_invalid".to_string() });
        let _ = server.retrieve_original(req_miss).await;
        assert_eq!(metrics.cache_misses.load(Ordering::Relaxed), 1);

        // Test server_info contains metrics JSON
        let req_info = Parameters(ServerInfoRequest {});
        let info = server.server_info(req_info).await.unwrap();
        assert!(info.contains("Metrics:"));
        assert!(info.contains("compressions_total"));

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_cache_align() {
        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: None,
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        let req = Parameters(CacheAlignRequest {
            chunks: vec!["chunk b".to_string(), "chunk a".to_string()],
            padding_size: Some(16),
        });

        let aligned = server.cache_align(req).await.unwrap();

        // Should sort alphabetically so chunk a comes first
        assert!(aligned.find("chunk a").unwrap() < aligned.find("chunk b").unwrap());

        // Verify structure and padding: chunk a (7 chars) + 9 spaces padding = 16 chars
        assert!(aligned.contains("chunk a         "));
        // chunk b (7 chars) + 9 spaces padding = 16 chars
        assert!(aligned.contains("chunk b         "));
        assert!(aligned.contains("<!-- chunk: "));
        assert!(aligned.contains("<!-- endchunk -->"));
    }

    #[tokio::test]
    async fn test_compress_schema() {
        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: None,
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        let schema_input = r#"{
            "title": "My Test Tool",
            "description": "A tool for testing purposes",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Item name"
                }
            }
        }"#;

        let req = Parameters(CompressSchemaRequest {
            schema: schema_input.to_string(),
        });

        let compressed = server.compress_schema(req).await.unwrap();
        // Check that description and title keys are stripped, leaving only minified structure
        assert!(!compressed.contains("description"));
        assert!(!compressed.contains("title"));
        assert!(compressed.contains("name"));
        assert!(compressed.contains("properties"));
        // Should be minified
        assert!(!compressed.contains("\n"));
    }

    #[tokio::test]
    async fn test_run_and_compress() {
        let temp_dir = std::env::temp_dir().join("headroom_test_run_cmd");
        fs::create_dir_all(&temp_dir).unwrap();

        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: Some(temp_dir.to_string_lossy().into_owned()),
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        // Run echo command
        let req = Parameters(RunAndCompressRequest {
            command: "echo".to_string(),
            args: Some(vec!["hello headroom".to_string()]),
        });

        let result = server.run_and_compress(req).await.unwrap();
        assert!(result.contains("hello headroom"));
        assert!(result.contains("CCR Ref: ccr_"));

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_compress_url_invalid() {
        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: None,
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        let req = Parameters(CompressUrlRequest {
            url: "http://invalid.url.local:12345/foo".to_string(),
        });

        let result = server.compress_url(req).await;
        // Should fail due to DNS lookup/connection failure
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_signatures_only_integration() {
        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: None,
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: false,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        let code_input = r#"
        pub struct Bar {
            val: usize,
        }
        impl Bar {
            pub fn run(&self) {
                let x = 123;
                println!("{}", x);
            }
        }
        "#;

        let req = Parameters(CompressContentRequest {
            raw_text: code_input.to_string(),
            content_type: "code".to_string(),
            threshold: None,
            preview: Some(true),
            signatures_only: Some(true),
        });

        let compressed = server.compress_content(req).await.unwrap();
        // Check that function body has been replaced with the ... placeholder
        assert!(compressed.contains("..."));
        // Struct fields should still be present
        assert!(compressed.contains("val: usize"));
    }

    #[test]
    fn test_dynamic_tool_schema_minifier() {
        let config = Arc::new(Config {
            log_threshold: 50_000,
            json_threshold: 10_000,
            max_input_size: 10 * 1024 * 1024,
            max_cache_bytes: 100 * 1024 * 1024,
            workspace_root: None,
            db_path: None,
            cache_ttl_hours: 0,
            metrics_interval: 0,
            compact_schemas: true,
        });
        let cache = Arc::new(MemoryCache::new(100 * 1024 * 1024));
        let server = HeadroomServer::new(config, cache, Arc::new(crate::metrics::Metrics::new()));

        // Inspect the schemas inside tool_router.map
        let route = server.tool_router.map.get("compress_content").unwrap();
        let schema_json = serde_json::to_string(&route.attr.input_schema).unwrap();

        // The input schema parameter descriptions (like "The raw string content to compress.")
        // should be stripped when compact_schemas is true
        assert!(!schema_json.contains("The raw string content to compress"));
        assert!(!schema_json.contains("description"));
    }
}

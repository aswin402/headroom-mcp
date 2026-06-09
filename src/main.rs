use std::collections::HashMap;
use std::sync::Mutex;
use std::path::Path;
use std::fs;
use rmcp::{tool, tool_router, ServiceExt, transport::stdio};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use rmcp::schemars::JsonSchema;
use regex::Regex;

#[derive(Clone)]
pub struct HeadroomServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    cache: std::sync::Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScopeContextRequest {
    #[schemars(description = "Absolute or relative path to the file/directory the agent is editing.")]
    pub target_path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompressContentRequest {
    #[schemars(description = "The raw string content to compress.")]
    pub raw_text: String,
    #[schemars(description = "The content type: 'json', 'code', or 'text_logs'.")]
    pub content_type: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RetrieveOriginalRequest {
    #[schemars(description = "The CCR ID (e.g. ccr_a1b2c) to retrieve.")]
    pub ccr_id: String,
}

fn mcp_error<E: std::fmt::Display>(err: E) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(err.to_string(), None)
}

#[tool_router(server_handler)]
impl HeadroomServer {
    pub fn new(cache: std::sync::Arc<Mutex<HashMap<String, String>>>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            cache,
        }
    }

    #[tool(description = "Walks up the directory tree and retrieves all relevant AGENTS.md instructions for the target file path.")]
    async fn scope_context(&self, req: Parameters<ScopeContextRequest>) -> Result<String, rmcp::ErrorData> {
        let path = Path::new(&req.0.target_path);
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_err(mcp_error)?.join(path)
        };

        // Resolve absolute path to canonical if possible, otherwise keep it as-is
        let resolved_path = absolute_path.canonicalize().unwrap_or(absolute_path);
        
        let target_dir = if resolved_path.is_dir() {
            resolved_path.as_path()
        } else {
            resolved_path.parent().unwrap_or(Path::new("/"))
        };

        let mut agents_files = Vec::new();
        let mut current_dir = Some(target_dir);

        while let Some(dir) = current_dir {
            let agents_path = dir.join("AGENTS.md");
            if agents_path.is_file() {
                agents_files.push(agents_path);
            }

            // Stop at git repository root or root file system
            if dir.join(".git").exists() {
                break;
            }

            current_dir = dir.parent();
        }

        // We want to combine from root/parent down to target directory
        agents_files.reverse();

        if agents_files.is_empty() {
            return Ok("No AGENTS.md files found in the path hierarchy.".to_string());
        }

        let mut combined_content = String::new();
        for file_path in agents_files {
            let content = fs::read_to_string(&file_path).map_err(mcp_error)?;
            let relative_path = file_path
                .strip_prefix(std::env::current_dir().unwrap_or_default())
                .unwrap_or(&file_path);
            combined_content.push_str(&format!(
                "### Context File: {}\n\n{}\n\n",
                relative_path.display(),
                content
            ));
        }

        Ok(combined_content)
    }

    #[tool(description = "Compresses logs, JSON, or code, and registers a CCR reference token for the agent.")]
    async fn compress_content(&self, req: Parameters<CompressContentRequest>) -> Result<String, rmcp::ErrorData> {
        let raw_text = req.0.raw_text.trim();
        if raw_text.is_empty() {
            return Ok("Empty content provided.".to_string());
        }

        // Generate a random CCR ID based on timestamp
        let time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let ccr_id = format!("ccr_{:x}", time_ns & 0xFFFFFFFF);

        // Store original text
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(ccr_id.clone(), req.0.raw_text.clone());
        }

        let compressed = match req.0.content_type.as_str() {
            "json" => compress_json(raw_text).map_err(mcp_error)?,
            "code" => compress_code(raw_text),
            _ => compress_logs(raw_text),
        };

        Ok(format!(
            "{} \n\n[CCR Ref: {} - call retrieve_original tool to inspect full content if needed]",
            compressed, ccr_id
        ))
    }

    #[tool(description = "Retrieves the original, uncompressed raw text for a given CCR reference ID.")]
    async fn retrieve_original(&self, req: Parameters<RetrieveOriginalRequest>) -> Result<String, rmcp::ErrorData> {
        let cache = self.cache.lock().unwrap();
        if let Some(content) = cache.get(&req.0.ccr_id) {
            Ok(content.clone())
        } else {
            Err(rmcp::ErrorData::internal_error(
                format!("CCR reference ID '{}' not found or expired.", req.0.ccr_id),
                None,
            ))
        }
    }
}

fn compress_json(raw_json: &str) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_str(raw_json)?;
    if let serde_json::Value::Array(arr) = value {
        if arr.is_empty() {
            return Ok("[]".to_string());
        }
        let total_count = arr.len();
        let mut keys = std::collections::BTreeSet::new();
        for item in &arr {
            if let serde_json::Value::Object(map) = item {
                for k in map.keys() {
                    keys.insert(k.clone());
                }
            }
        }
        
        let keys_str = keys.into_iter().collect::<Vec<String>>().join(", ");
        let first_item_str = serde_json::to_string_pretty(&arr[0]).unwrap_or_default();
        
        Ok(format!(
            "[CCR Summary: Array of {} objects. Keys: [{}]. \nFirst element:\n{}]",
            total_count, keys_str, first_item_str
        ))
    } else {
        let minified = serde_json::to_string(&value)?;
        if minified.len() > 1000 {
            Ok(format!("{}...", &minified[..1000]))
        } else {
            Ok(minified)
        }
    }
}

fn compress_code(raw_code: &str) -> String {
    let re_block = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let no_blocks = re_block.replace_all(raw_code, "");

    let re_line = Regex::new(r"//.*").unwrap();
    let no_comments = re_line.replace_all(&no_blocks, "");

    let re_lines = Regex::new(r"\n\s*\n").unwrap();
    let collapsed = re_lines.replace_all(&no_comments, "\n");

    collapsed.trim().to_string()
}

fn compress_logs(raw_logs: &str) -> String {
    let re_ansi = Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]").unwrap();
    let clean_logs = re_ansi.replace_all(raw_logs, "");

    if clean_logs.len() > 2000 {
        let first_part = &clean_logs[..1000];
        let last_part = &clean_logs[clean_logs.len() - 1000..];
        format!(
            "{}\n\n... [TRUNCATED LOGS - Use retrieve_original tool to view the full logs] ...\n\n{}",
            first_part, last_part
        )
    } else {
        clean_logs.into_owned()
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cache = std::sync::Arc::new(Mutex::new(HashMap::new()));
    let server = HeadroomServer::new(cache);
    
    // Start the stdio transport server
    server.serve(stdio()).await?;
    
    Ok(())
}
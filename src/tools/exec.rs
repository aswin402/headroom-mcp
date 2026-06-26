use std::sync::atomic::Ordering;
use crate::server::{HeadroomServer, mcp_error, log_info, COUNTER};
use crate::compression::logs::compress_logs;
use crate::intelligence::tokens::estimate_tokens;

pub async fn run_and_compress(
    server: &HeadroomServer,
    command: String,
    args: Option<Vec<String>>,
) -> Result<String, rmcp::ErrorData> {
    log_info(&format!("run_and_compress: cmd={}", command));

    // Get workspace root
    let workspace_root = std::fs::canonicalize(
        server.config.workspace_root.as_deref().unwrap_or(".")
    ).map_err(|e| mcp_error(format!("Failed to resolve workspace root: {}", e)))?;

    // Create the command
    let mut cmd = tokio::process::Command::new(&command);
    cmd.current_dir(&workspace_root);

    // Add arguments if provided
    if let Some(ref list) = args {
        cmd.args(list);
    }

    // Set standard environment settings for safety/reproducibility
    cmd.env("PAGER", "cat");

    // Execute command and capture output
    let output = cmd
        .output()
        .await
        .map_err(|e| mcp_error(format!("Failed to execute command '{}': {}", command, e)))?;

    // Combine stdout and stderr
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout_str, stderr_str);
    let trimmed = combined.trim();

    if trimmed.is_empty() {
        return Ok(format!("Command exited with status {}. No output was produced.", output.status));
    }

    if trimmed.len() > server.config.max_input_size {
        return Err(rmcp::ErrorData::internal_error(
            format!("Command output size ({} bytes) exceeds maximum allowed size of {} bytes", trimmed.len(), server.config.max_input_size),
            None,
        ));
    }

    // Compress using log compression
    let compressed = compress_logs(trimmed, server.config.log_threshold);

    // Cache full raw output
    let time_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);
    server.cache.insert(&id, trimmed, None).map_err(mcp_error)?;

    // Update metrics
    server.metrics.compressions_total.fetch_add(1, Ordering::Relaxed);
    server.metrics.total_bytes_compressed.fetch_add(trimmed.len() as u64, Ordering::Relaxed);
    server.metrics.total_bytes_saved.fetch_add(trimmed.len().saturating_sub(compressed.len()) as u64, Ordering::Relaxed);

    // Format output
    let original_tokens = estimate_tokens(trimmed);
    let compressed_tokens = estimate_tokens(&compressed);
    let saved_pct = if original_tokens > 0 {
        let saved = (original_tokens as f64 - compressed_tokens as f64) / original_tokens as f64 * 100.0;
        format!("{:.1}%", saved.max(0.0))
    } else {
        "0.0%".to_string()
    };

    let result = format!(
        "Command execution status: {}\n\n{}\n\n[CCR Ref: {} | Command: {} {:?} | Original: ~{} tokens | Compressed: ~{} tokens | Saved: {} | call retrieve_original to inspect full content]",
        output.status, compressed, id, command, args.unwrap_or_default(), original_tokens, compressed_tokens, saved_pct
    );

    Ok(result)
}

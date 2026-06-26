use std::time::Duration;
use std::sync::atomic::Ordering;
use crate::server::{HeadroomServer, mcp_error, log_info, COUNTER};
use crate::compression::json::compress_json;
use crate::compression::logs::compress_logs;
use crate::compression::detect::auto_detect_content_type;
use crate::intelligence::tokens::estimate_tokens;

pub async fn compress_url(
    server: &HeadroomServer,
    url: String,
) -> Result<String, rmcp::ErrorData> {
    log_info(&format!("compress_url: {}", url));

    // 1. Build HTTP client with 10-second timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| mcp_error(format!("Failed to build HTTP client: {}", e)))?;

    // 2. Fetch URL content
    let res = client
        .get(&url)
        .header("User-Agent", "headroom-mcp/0.5.0")
        .send()
        .await
        .map_err(|e| mcp_error(format!("Failed to fetch URL: {}", e)))?;

    // 3. Inspect headers for content length & content type
    if let Some(content_length) = res.content_length() {
        if content_length as usize > server.config.max_input_size {
            return Err(rmcp::ErrorData::internal_error(
                format!("URL content size ({} bytes) exceeds maximum allowed size of {} bytes", content_length, server.config.max_input_size),
                None,
            ));
        }
    }

    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|val| val.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    // 4. Retrieve raw text
    let raw_text = res
        .text()
        .await
        .map_err(|e| mcp_error(format!("Failed to read response body: {}", e)))?;

    if raw_text.len() > server.config.max_input_size {
        return Err(rmcp::ErrorData::internal_error(
            format!("Fetched content size ({} bytes) exceeds maximum allowed size of {} bytes", raw_text.len(), server.config.max_input_size),
            None,
        ));
    }

    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return Ok("URL returned empty content.".to_string());
    }

    // 5. Convert/Compress based on content type
    let (compressed, derived_type) = if content_type.contains("html") {
        log_info("compress_url: converting HTML to Markdown");
        let md = html2md::parse_html(trimmed);
        // Treat converted markdown as code/text
        (crate::compression::code::compress_code(&md), "markdown")
    } else if content_type.contains("json") {
        log_info("compress_url: compressing JSON response");
        let comp = compress_json(trimmed, server.config.json_threshold).map_err(mcp_error)?;
        (comp, "json")
    } else {
        log_info("compress_url: auto-detecting content type");
        let detected = auto_detect_content_type(trimmed);
        let comp = match detected {
            "json" => compress_json(trimmed, server.config.json_threshold).map_err(mcp_error)?,
            "code" | "yaml" | "markdown" => crate::compression::code::compress_code(trimmed),
            "csv" => crate::compression::csv::compress_csv(trimmed),
            _ => compress_logs(trimmed, server.config.log_threshold),
        };
        (comp, detected)
    };

    // 6. Cache original response (full text) if not preview (by default URLs are cached)
    let time_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let id = format!("ccr_{:x}_{:x}", time_ns & 0xFFFFFFFF, seq);
    server.cache.insert(&id, trimmed, None).map_err(mcp_error)?;

    // 7. Update metrics
    server.metrics.compressions_total.fetch_add(1, Ordering::Relaxed);
    server.metrics.total_bytes_compressed.fetch_add(trimmed.len() as u64, Ordering::Relaxed);
    server.metrics.total_bytes_saved.fetch_add(trimmed.len().saturating_sub(compressed.len()) as u64, Ordering::Relaxed);

    // 8. Format ratio and output
    let original_tokens = estimate_tokens(trimmed);
    let compressed_tokens = estimate_tokens(&compressed);
    let saved_pct = if original_tokens > 0 {
        let saved = (original_tokens as f64 - compressed_tokens as f64) / original_tokens as f64 * 100.0;
        format!("{:.1}%", saved.max(0.0))
    } else {
        "0.0%".to_string()
    };

    let result = format!(
        "{}\n\n[CCR Ref: {} | URL: {} | Type: {} | Original: ~{} tokens | Compressed: ~{} tokens | Saved: {} | call retrieve_original to inspect full content]",
        compressed, id, url, derived_type, original_tokens, compressed_tokens, saved_pct
    );

    Ok(result)
}

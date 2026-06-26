use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use crate::server::{HeadroomServer, log_info};

fn hash_string(s: &str) -> String {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub async fn cache_align(
    _server: &HeadroomServer,
    chunks: Vec<String>,
    padding_size: Option<usize>,
) -> Result<String, rmcp::ErrorData> {
    log_info(&format!("cache_align: aligning {} chunks", chunks.len()));

    let size = padding_size.unwrap_or(1024);
    if size == 0 {
        return Err(rmcp::ErrorData::internal_error(
            "Padding size must be greater than 0".to_string(),
            None,
        ));
    }

    // Sort chunks alphabetically to ensure deterministic ordering
    let mut sorted_chunks = chunks;
    sorted_chunks.sort();

    let mut aligned_output = String::new();

    for chunk in sorted_chunks {
        let trimmed = chunk.trim_end();
        let hash = hash_string(trimmed);

        let len = trimmed.len();
        let rem = len % size;
        let pad = if rem == 0 { 0 } else { size - rem };
        let padded = format!("{}{}", trimmed, " ".repeat(pad));

        aligned_output.push_str(&format!(
            "<!-- chunk: {} -->\n{}\n<!-- endchunk -->\n",
            hash, padded
        ));
    }

    Ok(aligned_output)
}

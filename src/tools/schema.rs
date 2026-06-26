use serde_json::Value;
use crate::server::{HeadroomServer, mcp_error, log_info};

fn minify_schema_val(val: &mut Value) {
    match val {
        Value::Object(map) => {
            // Remove description, title, examples
            map.remove("description");
            map.remove("title");
            map.remove("examples");

            // Recursively process child objects/arrays
            for (_, child) in map.iter_mut() {
                minify_schema_val(child);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                minify_schema_val(item);
            }
        }
        _ => {}
    }
}

pub async fn compress_schema(
    _server: &HeadroomServer,
    schema: String,
) -> Result<String, rmcp::ErrorData> {
    log_info("compress_schema: minifying JSON schema");

    let mut json_val: Value = serde_json::from_str(&schema)
        .map_err(|e| mcp_error(format!("Invalid JSON provided: {}", e)))?;

    minify_schema_val(&mut json_val);

    let minified = serde_json::to_string(&json_val)
        .map_err(|e| mcp_error(format!("Failed to serialize minified schema: {}", e)))?;

    Ok(minified)
}

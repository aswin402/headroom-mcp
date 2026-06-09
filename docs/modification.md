# Code Modification Guide

This guide explains how to extend or customize the **Headroom MCP** server, including adding new tools, customizing the directory-scoping logic, or adding new compression algorithms.

---

## 1. Adding a New Tool

To add a new tool, follow these three steps:

### Step A: Define the Input Parameters Struct
Define a parameters struct implementing `serde::Deserialize` and `rmcp::schemars::JsonSchema`. Use `#[schemars(description = "...")]` to describe fields for the LLM.
```rust
#[derive(Deserialize, JsonSchema)]
pub struct CustomToolRequest {
    #[schemars(description = "Description of parameter")]
    pub my_param: String,
}
```

### Step B: Add the Handler Method
Add an async method inside the `#[tool_router(server_handler)] impl HeadroomServer` block, decorated with the `#[tool]` attribute:
```rust
#[tool(description = "Explain what this tool does to the LLM")]
async fn custom_tool(&self, req: Parameters<CustomToolRequest>) -> Result<String, rmcp::ErrorData> {
    let param = &req.0.my_param;
    // Perform logic...
    Ok(format!("Result of tool processing: {}", param))
}
```

The macro will automatically update `list_tools` and `call_tool` routing definitions on compile.

---

## 2. Modifying the Context Scoping Logic

Currently, `scope_context` walks parent directories searching specifically for `AGENTS.md` files:
*   To search for files other than `AGENTS.md` (e.g., `CLAUDE.md` or `.rules`), modify this line inside `scope_context` in `src/main.rs`:
    ```rust
    let agents_path = dir.join("AGENTS.md");
    ```
*   To ignore specific folders (like `node_modules` or `target`), or limit the maximum tree depth, add checks inside the `while let Some(dir) = current_dir` loop.

---

## 3. Extending the Compression Algorithms

To add support for a new data format (e.g., Markdown tables or CSVs):

### Step A: Register the Content Type
Update `CompressContentRequest`'s description enum or validation:
```rust
#[derive(Deserialize, JsonSchema)]
pub struct CompressContentRequest {
    ...
    // add to content_type options: "csv"
}
```

### Step B: Add a Custom Compressor Function
Implement a minifier function at the bottom of `src/main.rs`:
```rust
fn compress_csv(raw_csv: &str) -> String {
    // Write logic to parse headers, count rows, and output only the first 3 rows
}
```

### Step C: Route the Compressor
Wire it inside `compress_content`'s match block:
```rust
let compressed = match req.0.content_type.as_str() {
    "json" => compress_json(raw_text).map_err(mcp_error)?,
    "code" => compress_code(raw_text),
    "csv" => compress_csv(raw_text), // New route
    _ => compress_logs(raw_text),
};
```

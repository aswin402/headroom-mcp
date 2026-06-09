# Code Structure & Reference

This document maps out the files in the project and explains how the core logic in `src/main.rs` is organized.

---

## File Layout

```
agentcpower/
├── Cargo.toml          # Rust package manager configuration
├── README.md           # Master documentation and overview
├── assets/
│   └── logo.svg        # Animated logo asset
├── docs/
│   ├── architecture.md # Structural layout documentation
│   ├── codes.md        # Code explanation (this file)
│   ├── modification.md # Guide to customizing the codebase
│   └── usage.md        # Command descriptions and config settings
├── onpkg.json          # AI agent manifest configuration
├── onpkg_docs/
│   └── rust.md         # Rust coding conventions skill file
└── src/
    └── main.rs         # Complete server implementation
```

---

## Code Breakdown (`src/main.rs`)

The entire server is contained in `src/main.rs` for maximum portability and simplicity. It consists of the following sections:

### 1. Imports and Struct Definition
```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::path::Path;
use std::fs;
...
```
*   **`HeadroomServer`:** Holds the generated `ToolRouter` (which handles the method registration) and the thread-safe session cache.

### 2. Request Structs
All inputs passed to our tools must be wrapped in structured schemas using `rmcp::handler::server::wrapper::Parameters`:
*   `ScopeContextRequest`: Maps the absolute or relative target file path.
*   `CompressContentRequest`: Holds the string content and its category (`json`, `code`, `text_logs`).
*   `RetrieveOriginalRequest`: Holds the `ccr_id` key for reversing compressed tokens.

### 3. Tool Implementations (inside `impl HeadroomServer`)
The `#[tool_router(server_handler)]` macro parses this block and generates standard JSON-RPC handlers for:
*   `scope_context`: Performs the parent-directory walker, collecting and concatenating `AGENTS.md` files.
*   `compress_content`: Caches the raw content, runs it through the matching minifier function, and returns a formatted CCR reference.
*   `retrieve_original`: Looks up the reference key in the thread-safe `HashMap` cache.

### 4. Minification Utilities (Helper Functions)
*   `compress_json`: Deserializes JSON, checks if it's an array, lists unique keys, and retains only the first element as a template.
*   `compress_code`: Uses regex to strip out syntax comments and collapse extra whitespace lines.
*   `compress_logs`: Strips ANSI shell formatting codes and slices out the middle of massive logs, leaving only a small header and tail.

### 5. main Function
*   Initializes the session `HashMap` wrapped in `Arc<Mutex<...>>` to allow safe sharing across multiple concurrent threads.
*   Spawns `HeadroomServer` with the cache ref.
*   Runs `server.serve(stdio()).await?` to bootstrap the standard I/O listener loop.

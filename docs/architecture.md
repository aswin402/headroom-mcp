# Architecture Design

This document details the architectural layout, core modules, and data flow of the **Headroom MCP** server.

---

## High-Level Modules

```
┌────────────────────────────────────────────────────────┐
│                   Headroom MCP Binary                  │
│                                                        │
│   ┌────────────────────────────────────────────────┐   │
│   │               rmcp SDK Stdio Loop              │   │
│   └───────────────────────┬────────────────────────┘   │
│                           │ JSON-RPC Requests          │
│                           ▼                            │
│   ┌────────────────────────────────────────────────┐   │
│   │           HeadroomServer (ToolRouter)          │   │
│   └───────┬───────────────┬────────────────┬───────┘   │
│           │               │                │           │
│           ▼               ▼                ▼           │
│    ┌─────────────┐ ┌─────────────┐  ┌─────────────┐    │
│    │    DOX      │ │ Compression │  │  CCR Cache  │    │
│    │ Directory   │ │  Pipelines  │  │ In-Memory   │    │
│    │   Walker    │ │ (JSON/Code/│  │  HashMap    │    │
│    │             │ │    Logs)    │  │             │    │
│    └──────┬──────┘ └──────┬──────┘  └──────┬──────┘    │
│           │               │                │           │
└───────────┼───────────────┼────────────────┼───────────┘
            ▼               ▼                ▼
     [Local Files]   [Minified Output] [Session Keys]
```

---

## 1. Stdio Transport Layer
The server runs as a local background process communicating with the client (such as Claude Desktop or Claude Code) over standard input (`stdin`) and standard output (`stdout`) using JSON-RPC 2.0 messages. 
*   **Safety Constraints:** Because `stdout` is the primary transport pipe for protocol messages, all logging, debugging messages, or panic traces must be routed to `stderr` or completely silenced to prevent JSON-RPC transport corruption.
*   **Concurrency:** The server runs on a multi-threaded `tokio` runtime, handling incoming JSON-RPC calls concurrently while preserving safe concurrent access to the session memory.

---

## 2. Tool Router & Dispatching
We leverage the official `rmcp` macro framework. The `#[tool_router(server_handler)]` macro on our `HeadroomServer` implementation block does the following:
*   Automatically implements the `ServerHandler` trait for the server struct.
*   Generates a tool routing table mapping incoming JSON-RPC `tools/call` requests to the matching asynchronous methods.
*   Automatically validates parameters and generates OpenAPI/JSON Schemas using `schemars` v1.2, ensuring the LLM understands exactly how to invoke each tool.

---

## 3. In-Memory Session Cache
For Reversible Compression (CCR), the raw uncompressed text of logs, code, or search responses must be cached.
*   **Lifecycle:** Because the MCP server runs as a child process of the agent session, the cache lifecycle is naturally tied to the agent session. A simple, fast, thread-safe in-memory cache (`Arc<Mutex<HashMap<String, String>>>`) is used.
*   **Speed:** Retrieval operations take less than a microsecond, avoiding any filesystem or database I/O overhead.

---

## 4. Scoping & Compression Pipeline

### Context Scoping (DOX)
*   The `scope_context` tool takes a target file path, converts it to an absolute path, and walks upwards through parent folders.
*   It stops at the filesystem root `/` or when it detects a `.git` folder (representing the repository root boundary).
*   It reads all nested `AGENTS.md` files along that path, combining them from root to leaf to give the agent scoped, inherited instructions.

### Compression Engines
*   **JSON Array Crusher:** Detects if a JSON payload is an array of objects. If so, it extracts the unique fields, outputs the count, and formats just the first object as a structural template.
*   **AST Code Minifier:** Operates deterministically by using regular expressions to strip out block comments (`/* ... */`), line comments (`// ...`), and collapse redundant empty lines.
*   **Log Purger:** Strips ANSI escape colors and truncates massive logs, leaving only a 1000-character header and 1000-character tail to keep the error context while saving 95% of token space.

# Usage & Integration Guide

This guide explains how to start, debug, and utilize the **Headroom MCP** server in your development environments.

---

## 1. Running the Server

Because the transport layer works over standard I/O (stdio), running the binary directly in a terminal will wait for JSON-RPC messages and will not print human-readable text.

To test compilation and start in developer mode:
```bash
cargo run
```

To build a release-optimized binary:
```bash
cargo build --release
```
The resulting executable is generated at `target/release/headroom-mcp`.

---

## 2. Integration Configs

Configure your agent clients to launch the binary:

### Claude Desktop Integration
Add the following to your desktop config file:
```json
{
  "mcpServers": {
    "headroom-mcp": {
      "command": "/path/to/project/target/release/headroom-mcp",
      "args": []
    }
  }
}
```

### Claude Code Integration
Add the local MCP server via CLI:
```bash
claude mcp add headroom-mcp /path/to/project/target/release/headroom-mcp
```

---

## 3. Tool Reference & Examples

### Tool: `scope_context`
Allows the agent to search folder-specific instructions.

*   **Example Input:**
    ```json
    {
      "target_path": "./src/auth/login.rs"
    }
    ```
*   **Example Response:**
    ```markdown
    ### Context File: ./AGENTS.md
    - Enforce standard formatting rules.
    - Keep dependencies inside cargo lock.

    ### Context File: ./src/auth/AGENTS.md
    - Use token hashing.
    - Avoid saving clear-text credentials in logs.
    ```

---

### Tool: `compress_content`
Strips redundant tokens from verbose items.

*   **Example Input (Log Files):**
    ```json
    {
      "raw_text": "[11:15:30] Compiling auth\n[11:15:32] Error: division by zero at main.rs:15\n[11:15:33] Compilation failed...",
      "content_type": "text_logs"
    }
    ```
*   **Example Response:**
    ```
    [11:15:30] Compiling auth
    [11:15:32] Error: division by zero at main.rs:15
    [11:15:33] Compilation failed...

    [CCR Ref: ccr_72fa11 - call retrieve_original tool to inspect full content if needed]
    ```

---

### Tool: `retrieve_original`
Retrieves original, uncompressed data.

*   **Example Input:**
    ```json
    {
      "ccr_id": "ccr_72fa11"
    }
    ```
*   **Example Response:**
    ```
    [11:15:30] Compiling auth
    [11:15:32] Error: division by zero at main.rs:15
    [11:15:33] Compilation failed...
    ```

---

## 4. Debugging & Logs
*   **Inspecting:** You can use the official `@modelcontextprotocol/inspector` tool to run a GUI interface to inspect tools and run manual calls:
    ```bash
    npx -y @modelcontextprotocol/inspector target/release/headroom-mcp
    ```
*   **Stdio Safety:** If you write any standard debug printouts (like `println!`), make sure to write them to `stderr` (`eprintln!`) so they do not corrupt the stdio stream.

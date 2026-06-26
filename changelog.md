# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-06-26

### Added
- **LRU Cache Eviction:** Added size-based cache eviction to bound memory utilization of the in-memory cache to 100MB maximum footprint.
- **Log Line Deduplication:** Added deduplication of consecutive duplicate log lines in `compress_logs` to replace redundant repeats with a clean `[repeated N times]` message.
- **Auto-detection of Content Types:** Support for `"auto"` or empty content types, dynamically resolving type from file extensions or content structural patterns.
- **Markdown, CSV, and YAML Support:** Added specialized compressors for Markdown and CSV files (showing headers + first 3 rows), and mapping YAML directly to comments stripping.
- **New MCP Tools:**
  - `compress_file`: Reads, detects content type, compresses, and caches directly from file paths.
  - `cache_stats`: Returns total cached items, size in bytes, and lists keys.
  - `clear_cache`: Evicts all cached entries to free memory.
  - `server_info`: Displays version, uptime, cache size, and configuration settings.
- **Diagnostics:** Stderr-based structured logging for operations like tool execution, cache eviction, and blocking traversal attempts.
- **Unit Test Suite:** A comprehensive test suite with 9 unit tests for all UTF-8 safety helpers, content auto-detection, log deduplication, and file extension checking.

### Fixed
- **UTF-8 Panics (BUG-01/02):** Replaced unsafe byte slicing (`&string[..1000]`) with a custom character-boundary-safe `safe_truncate` and `safe_tail` helper which iterates using `char_indices()`.
- **Mutex Poisoning (BUG-03):** Replaced `.unwrap()` lock requests with `unwrap_or_else(|poisoned| poisoned.into_inner())` to recover cache access even after previous thread failures.
- **ID Collisions (BUG-04):** Replaced the 32-bit timestamp sequence generator with a thread-safe global `AtomicU64` counter combined with a timestamp segment (`ccr_{timestamp_hex}_{counter_hex}`) to guarantee uniqueness under rapid concurrent cache insertions.
- **Hardcoded Thresholds (HARD-01/02/03):** Raised default thresholds significantly to accommodate large context windows (e.g. 128k):
  - Logs: 2,000 characters $\rightarrow$ 50,000 characters default threshold.
  - JSON: 1,000 characters $\rightarrow$ 10,000 characters default threshold.
  - Head/Tail retention: 1,000 characters $\rightarrow$ 5,000 characters each.
- **Optional Request Thresholds (HARD-05):** Added support for an optional `threshold` field in request payloads so agents can dynamically customize thresholds.
- **Security Traversal (SEC-01/02):** Restructured `retrieve_original` and `compress_file` file system reads to canonicalize paths and ensure they do not escape the workspace directory via path traversal (e.g. using `starts_with(&workspace_root)`).
- **Max Input Limit (SEC-03):** Added `MAX_INPUT_SIZE` (10MB limit) check on all inputs to protect the cache from memory exhaustion attacks.
- **Multi-File Scoping (HARD-04/FEAT-09):** `scope_context` now searches for `AGENTS.md`, `CLAUDE.md`, `CURSOR.md`, and `.cursorrules` hierarchically.
- **Language comment compatibility (ARCH-04):** Enhanced code compressor regex to correctly handle comments in C/C++/Java/Rust (`//`, `/* */`), Python/Shell (`#`), SQL (`--`), and HTML (`<!-- -->`) without stripping URLs or strings.

### Changed
- **Modular Project Architecture (ARCH-01):** Refactored the single-file `src/main.rs` layout into modular sub-files:
  - `src/config.rs` — Constants & Default settings.
  - `src/cache.rs` — Cache structures.
  - `src/compression.rs` — Compression logic, UTF-8 safety, and unit tests.
  - `src/server.rs` — MCP Server implementation and tool router definition.
  - `src/main.rs` — Minimal entrypoint.
- **Dependencies cleaned (DEP-01/02/04):** Removed unused dependencies (`walkdir`, `clap`) from `Cargo.toml` and updated `onpkg.json`.
- **Regex Performance (DEP-03):** Compiled Regex patterns once using `LazyLock` instead of re-compiling on every function call.

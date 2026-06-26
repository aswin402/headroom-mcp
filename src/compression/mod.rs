/// Context compression engines and submodules.

pub mod errors;
pub mod diff;
pub mod helpers;
pub mod detect;
pub mod json;
pub mod code;
pub mod csv;
pub mod logs;

// Re-export all public functions to preserve the public API.
#[allow(unused_imports)]
pub use helpers::{safe_tail, safe_truncate};
pub use detect::{auto_detect_content_type, detect_content_type_from_ext};
pub use json::compress_json;
pub use code::compress_code;
pub use csv::compress_csv;
#[allow(unused_imports)]
pub use logs::{compress_logs, deduplicate_log_lines};

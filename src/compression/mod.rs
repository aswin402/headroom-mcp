/// Context compression engines and submodules.

pub mod errors;
pub mod diff;
pub mod helpers;
pub mod detect;
pub mod json;
pub mod code;
pub mod csv;
pub mod logs;
pub mod syntax;
pub mod cmd_filters;

// Re-export all public functions to preserve the public API.
#[allow(unused_imports)]
pub use cmd_filters::filter_command_output;
#[allow(unused_imports)]
pub use helpers::{safe_tail, safe_truncate};
#[allow(unused_imports)]
pub use detect::{auto_detect_content_type, detect_content_type_from_ext};
#[allow(unused_imports)]
pub use json::compress_json;
#[allow(unused_imports)]
pub use code::{compress_code, compress_code_with_options};
#[allow(unused_imports)]
pub use csv::compress_csv;
#[allow(unused_imports)]
pub use logs::{compress_logs, deduplicate_log_lines};
#[allow(unused_imports)]
pub use syntax::extract_signatures;

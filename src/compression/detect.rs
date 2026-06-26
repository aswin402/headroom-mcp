/// Content type detection.

use std::path::Path;

pub fn auto_detect_content_type(raw_text: &str) -> &str {
    let trimmed = raw_text.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if serde_json::from_str::<serde_json::Value>(raw_text).is_ok() {
            return "json";
        }
    }
    
    if trimmed.contains("\x1B[") || trimmed.lines().take(10).any(|line| {
        line.contains("INFO") || line.contains("ERROR") || line.contains("WARN") || line.contains("DEBUG")
    }) {
        return "text_logs";
    }
    "code"
}

pub fn detect_content_type_from_ext(path: &Path) -> Option<&'static str> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_lowercase().as_str() {
            "json" => Some("json"),
            "csv" => Some("csv"),
            "md" | "markdown" => Some("markdown"),
            "yml" | "yaml" => Some("yaml"),
            "log" | "txt" => Some("text_logs"),
            "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "go" | "java" | "sh" | "sql" | "html" | "css" => Some("code"),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_detect_content_type() {
        assert_eq!(auto_detect_content_type("  {\"key\": \"val\"}  "), "json");
        assert_eq!(auto_detect_content_type("  [1, 2, 3]  "), "json");
        assert_eq!(auto_detect_content_type("2026-06-26 INFO: startup complete"), "text_logs");
        assert_eq!(auto_detect_content_type("some regular code();"), "code");
    }

    #[test]
    fn test_detect_content_type_from_ext() {
        assert_eq!(detect_content_type_from_ext(Path::new("test.json")), Some("json"));
        assert_eq!(detect_content_type_from_ext(Path::new("test.csv")), Some("csv"));
        assert_eq!(detect_content_type_from_ext(Path::new("test.rs")), Some("code"));
        assert_eq!(detect_content_type_from_ext(Path::new("test.log")), Some("text_logs"));
        assert_eq!(detect_content_type_from_ext(Path::new("test.xyz")), None);
    }
}

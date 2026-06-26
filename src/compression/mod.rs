use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

pub mod errors;

// --- Static Regex patterns ---
static RE_BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static RE_HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
static RE_LINE_COMMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:\s+|^)(?://|#|--).*$").unwrap()
});
static RE_BLANK_LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());
static RE_ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]").unwrap());

// --- Helper functions for UTF-8 safe slicing ---
pub fn safe_truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

pub fn safe_tail(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match s.char_indices().rev().nth(max_chars - 1) {
        Some((idx, _)) => &s[idx..],
        None => s,
    }
}

// --- Content type auto detection ---
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

// --- Deduplication of consecutive duplicate log lines ---
pub fn deduplicate_log_lines(logs: &str) -> String {
    let mut result = String::with_capacity(logs.len());
    let mut lines = logs.lines().peekable();
    
    while let Some(line) = lines.next() {
        let mut count = 1;
        while let Some(&next_line) = lines.peek() {
            if next_line == line {
                count += 1;
                lines.next();
            } else {
                break;
            }
        }
        
        if count > 1 {
            result.push_str(line);
            result.push_str(&format!(" [repeated {} times]\n", count));
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    
    if !logs.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    
    result
}

// --- Specific Compressors ---
pub fn compress_json(raw_json: &str, threshold: usize) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_str(raw_json)?;
    if let serde_json::Value::Array(arr) = value {
        if arr.is_empty() {
            return Ok("[]".to_string());
        }
        let total_count = arr.len();
        let mut keys = std::collections::BTreeSet::new();
        for item in &arr {
            if let serde_json::Value::Object(map) = item {
                for k in map.keys() {
                    keys.insert(k.clone());
                }
            }
        }

        let keys_str = keys.into_iter().collect::<Vec<String>>().join(", ");
        let first_item_str = serde_json::to_string_pretty(&arr[0]).unwrap_or_default();

        Ok(format!(
            "[CCR Summary: Array of {} objects. Keys: [{}]. \nFirst element:\n{}]",
            total_count, keys_str, first_item_str
        ))
    } else {
        let minified = serde_json::to_string(&value)?;
        if minified.char_indices().nth(threshold).is_some() {
            Ok(format!("{}...", safe_truncate(&minified, threshold)))
        } else {
            Ok(minified)
        }
    }
}

pub fn compress_code(raw_code: &str) -> String {
    let no_blocks = RE_BLOCK_COMMENT.replace_all(raw_code, "");
    let no_html = RE_HTML_COMMENT.replace_all(&no_blocks, "");
    let no_comments = RE_LINE_COMMENTS.replace_all(&no_html, "");
    let collapsed = RE_BLANK_LINES.replace_all(&no_comments, "\n");
    collapsed.trim().to_string()
}

pub fn compress_csv(raw_csv: &str) -> String {
    let mut lines = raw_csv.lines();
    let mut result = String::new();
    if let Some(header) = lines.next() {
        result.push_str(&format!("Headers: {}\n", header));
    }
    let mut count = 0;
    for line in lines.by_ref() {
        if count < 3 {
            result.push_str(&format!("Row {}: {}\n", count + 1, line));
        }
        count += 1;
    }
    result.push_str(&format!("[CCR Summary: CSV contains {} rows total]", count + 1));
    result
}

pub fn compress_logs(raw_logs: &str, threshold: usize) -> String {
    let clean_logs = RE_ANSI.replace_all(raw_logs, "");
    let deduped_logs = deduplicate_log_lines(&clean_logs);

    let count_exceeds = deduped_logs.char_indices().nth(threshold).is_some();
    if count_exceeds {
        let lines: Vec<&str> = deduped_logs.lines().collect();
        if lines.len() <= 15 {
            return deduped_logs;
        }

        // Keep first 5 and last 5 lines
        let head_lines = &lines[..5];
        let tail_lines = &lines[lines.len() - 5..];
        let middle_lines = &lines[5..lines.len() - 5];

        // Let's compute characters available for middle lines:
        let head_len: usize = head_lines.iter().map(|l| l.len() + 1).sum();
        let tail_len: usize = tail_lines.iter().map(|l| l.len() + 1).sum();
        
        let remaining_budget_chars = threshold.saturating_sub(head_len + tail_len + 100); // 100 char buffer
        
        // Average line length is about 100 chars, estimate budget in lines
        let line_budget = remaining_budget_chars / 100;
        let line_budget = line_budget.max(5).min(middle_lines.len());

        let important = crate::intelligence::scoring::select_important_lines(middle_lines, line_budget);
        
        let mut result = String::new();
        for l in head_lines {
            result.push_str(l);
            result.push('\n');
        }
        
        result.push_str("\n... [TRUNCATED LOGS - keeping important lines below] ...\n\n");
        
        let mut last_idx = 0;
        for (idx, line) in important {
            if idx > last_idx + 1 && last_idx > 0 {
                result.push_str("... [skipped lines] ...\n");
            }
            result.push_str(line);
            result.push('\n');
            last_idx = idx;
        }

        result.push_str("\n... [end of truncated section] ...\n\n");

        for l in tail_lines {
            result.push_str(l);
            result.push('\n');
        }
        
        if result.ends_with('\n') {
            result.pop();
        }
        result
    } else {
        deduped_logs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_truncate() {
        assert_eq!(safe_truncate("hello", 10), "hello");
        assert_eq!(safe_truncate("hello", 3), "hel");
        assert_eq!(safe_truncate("hello", 0), "");
        // UTF-8 multi-byte characters
        assert_eq!(safe_truncate("🦀🦀🦀🦀🦀", 2), "🦀🦀");
        assert_eq!(safe_truncate("A🦀B", 2), "A🦀");
    }

    #[test]
    fn test_safe_tail() {
        assert_eq!(safe_tail("hello", 10), "hello");
        assert_eq!(safe_tail("hello", 3), "llo");
        assert_eq!(safe_tail("hello", 0), "");
        // UTF-8 multi-byte characters
        assert_eq!(safe_tail("🦀🦀🦀🦀🦀", 2), "🦀🦀");
        assert_eq!(safe_tail("A🦀B", 2), "🦀B");
    }

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

    #[test]
    fn test_deduplicate_log_lines() {
        let logs = "line1\nline1\nline1\nline2\nline2\nline3";
        let expected = "line1 [repeated 3 times]\nline2 [repeated 2 times]\nline3";
        assert_eq!(deduplicate_log_lines(logs), expected);
    }

    #[test]
    fn test_compress_json() {
        // Empty array
        assert_eq!(compress_json("[]", 10).unwrap(), "[]");
        
        // Single object inside array
        let single_obj = r#"[{"name": "test", "id": 1}]"#;
        let res = compress_json(single_obj, 10).unwrap();
        assert!(res.contains("Array of 1 objects"));
        assert!(res.contains("id, name"));
        
        // Non-array minified JSON
        let non_arr = r#"{"name": "hello", "nested": {"val": 123}}"#;
        let res_non_arr = compress_json(non_arr, 100).unwrap();
        assert_eq!(res_non_arr, "{\"name\":\"hello\",\"nested\":{\"val\":123}}");
        
        // Non-array JSON truncation
        let res_trunc = compress_json(non_arr, 15).unwrap();
        assert_eq!(res_trunc, "{\"name\":\"hello\"...");
    }

    #[test]
    fn test_compress_code() {
        // C-style comments
        let code = "let x = 1; // comment\n/* block\ncomment */\nlet y = 2;";
        let expected = "let x = 1;\nlet y = 2;";
        assert_eq!(compress_code(code), expected);

        // Python/SQL style comments
        let code_py = "x = 1 # python comment\n# whole line comment\ny = 2 -- sql comment";
        let expected_py = "x = 1\ny = 2";
        assert_eq!(compress_code(code_py), expected_py);
        
        // Make sure URLs are not stripped
        let code_url = "let url = \"https://google.com\";\n# comment";
        assert_eq!(compress_code(code_url), "let url = \"https://google.com\";");
    }

    #[test]
    fn test_compress_csv() {
        let csv = "id,name,age\n1,alice,30\n2,bob,25\n3,charlie,35\n4,david,40";
        let res = compress_csv(csv);
        assert!(res.contains("Headers: id,name,age"));
        assert!(res.contains("Row 1: 1,alice,30"));
        assert!(res.contains("CSV contains 5 rows total"));
    }

    #[test]
    fn test_compress_logs() {
        // ANSI escape codes
        let logs = "\x1B[31mError:\x1B[0m something went wrong";
        assert_eq!(compress_logs(logs, 100), "Error: something went wrong");

        // Logs truncation
        let long_logs = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\nline11\nline12\nline13\nline14\nline15\nline16";
        let res = compress_logs(long_logs, 50);
        assert!(res.contains("TRUNCATED LOGS"));
    }
}

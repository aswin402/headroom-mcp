/// Log parsing, deduplication, and truncation based on importance.

use regex::Regex;
use std::sync::LazyLock;

static RE_ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1B\[[0-9;]*[a-zA-Z]").unwrap());

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
    fn test_deduplicate_log_lines() {
        let logs = "line1\nline1\nline1\nline2\nline2\nline3";
        let expected = "line1 [repeated 3 times]\nline2 [repeated 2 times]\nline3";
        assert_eq!(deduplicate_log_lines(logs), expected);
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

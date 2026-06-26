/// Scores a single log line by importance (0.0 to 1.0)
pub fn score_log_line(line: &str) -> f32 {
    let mut score: f32 = 0.1; // baseline
    let lower = line.to_lowercase();

    // Critical keywords
    if lower.contains("error") || lower.contains("fatal") || lower.contains("panic") {
        score += 0.5;
    }
    if lower.contains("failed") || lower.contains("failure") || lower.contains("exception") {
        score += 0.4;
    }
    // Warning keywords
    if lower.contains("warning") || lower.contains("warn") || lower.contains("deprecated") {
        score += 0.2;
    }
    // File paths & line numbers (usually near errors)
    if lower.contains(".rs:") || lower.contains(".py:") || lower.contains(".js:")
       || lower.contains(".ts:") || lower.contains("line ") {
        score += 0.3;
    }
    // Stack trace frames
    if lower.contains("at ") || lower.starts_with("  ") || lower.contains("backtrace") {
        score += 0.2;
    }

    score.min(1.0)
}

/// Given a set of log lines and a budget, return the most important lines
pub fn select_important_lines<'a>(lines: &[&'a str], budget: usize) -> Vec<(usize, &'a str)> {
    let mut scored: Vec<(usize, f32, &'a str)> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| (i, score_log_line(line), *line))
        .collect();

    // Sort by score descending, keep top N
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(budget);

    // Re-sort by original position for readability
    scored.sort_by_key(|&(i, _, _)| i);
    scored.iter().map(|&(i, _, line)| (i, line)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_log_line() {
        assert!(score_log_line("ERROR: division by zero") > 0.5);
        assert!(score_log_line("info: starting server") < 0.2);
        assert!(score_log_line("at main.rs:12:3") > 0.3);
    }

    #[test]
    fn test_select_important_lines() {
        let lines = vec![
            "INFO: starting test suite",
            "WARNING: slow database query detected",
            "INFO: processing request 12",
            "ERROR: connection refused at db.rs:42",
            "INFO: request 12 completed in 12ms",
        ];

        let selected = select_important_lines(&lines, 2);
        assert_eq!(selected.len(), 2);
        // Should keep warning and error
        assert_eq!(selected[0].1, "WARNING: slow database query detected");
        assert_eq!(selected[1].1, "ERROR: connection refused at db.rs:42");
    }
}

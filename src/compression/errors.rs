use regex::Regex;
use std::sync::LazyLock;

static RUST_ERR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^(error\[E\d+\]:.*|warning:.*|-->.*:\d+:\d+)").unwrap()
});

static PY_ERR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)(Traceback \(most recent call last\):|File ".*", line \d+, in .*)|\w+Error:.*"#).unwrap()
});

static JS_ERR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)(at .* \([^)]+:\d+:\d+\)|at .*:\d+:\d+|\w+Error: .*)").unwrap()
});

static GO_ERR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^.*\.go:\d+:\d+:.*").unwrap()
});

/// Extracts structured error messages from build logs or test outputs
pub fn extract_errors(raw_output: &str) -> String {
    let mut extracted = Vec::new();
    let lines: Vec<&str> = raw_output.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        
        let is_error = RUST_ERR_RE.is_match(line)
            || PY_ERR_RE.is_match(line)
            || JS_ERR_RE.is_match(line)
            || GO_ERR_RE.is_match(line)
            || line.to_lowercase().contains("panic!")
            || line.to_lowercase().contains("failed")
            || line.to_lowercase().contains("exception");

        if is_error {
            // Keep the error line
            extracted.push(line.to_string());
            
            // Optionally add next line as context if it exists and isn't another error
            if i + 1 < lines.len() {
                let next_line = lines[i + 1];
                let is_next_error = RUST_ERR_RE.is_match(next_line)
                    || PY_ERR_RE.is_match(next_line)
                    || JS_ERR_RE.is_match(next_line)
                    || GO_ERR_RE.is_match(next_line);
                if !is_next_error && !next_line.trim().is_empty() {
                    extracted.push(format!("  {}", next_line.trim()));
                    i += 1;
                }
            }
        }
        i += 1;
    }

    if extracted.is_empty() {
        // Return first 5 and last 5 lines if no specific errors detected
        if lines.len() <= 10 {
            raw_output.to_string()
        } else {
            let mut fallback = Vec::new();
            for &l in &lines[..5] {
                fallback.push(l.to_string());
            }
            fallback.push("... [truncated] ...".to_string());
            for &l in &lines[lines.len() - 5..] {
                fallback.push(l.to_string());
            }
            fallback.join("\n")
        }
    } else {
        extracted.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_errors() {
        let log = "\
cargo check
warning: unused import
error[E0308]: mismatched types
  --> src/main.rs:12:9
   |
12 |     let x: u32 = \"hello\";
   |                  ^^^^^^^ expected `u32`, found `&str`
";
        let extracted = extract_errors(log);
        assert!(extracted.contains("error[E0308]"));
        assert!(extracted.contains("src/main.rs:12:9"));
    }

    #[test]
    fn test_extract_python_errors() {
        let log = "\
Traceback (most recent call last):
  File \"app.py\", line 10, in <module>
    func()
  File \"app.py\", line 5, in func
    1 / 0
ZeroDivisionError: division by zero
";
        let extracted = extract_errors(log);
        assert!(extracted.contains("Traceback"));
        assert!(extracted.contains("ZeroDivisionError"));
    }
}

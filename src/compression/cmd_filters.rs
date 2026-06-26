pub fn filter_command_output(command: &str, raw: &str, threshold: usize) -> String {
    let base_cmd = command.split_whitespace().next().unwrap_or("");
    let base_name = std::path::Path::new(base_cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(base_cmd)
        .to_lowercase();

    let filtered = match base_name.as_str() {
        "cargo" | "rustc" => filter_cargo_output(raw, threshold),
        "npm" | "npx" | "yarn" | "pnpm" | "bun" => filter_npm_output(raw, threshold),
        "git" => filter_git_output(raw, threshold),
        "python" | "python3" | "pytest" | "pip" | "pip3" => filter_python_output(raw, threshold),
        _ => return crate::compression::logs::compress_logs(raw, threshold),
    };

    if filtered.len() > threshold {
        crate::compression::logs::compress_logs(&filtered, threshold)
    } else {
        filtered
    }
}

fn filter_cargo_output(raw: &str, _threshold: usize) -> String {
    let mut result = Vec::new();
    let mut warning_count = 0;
    let mut omitted_warnings = 0;

    for line in raw.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("Compiling ") || trimmed_start.starts_with("Downloading ") {
            continue;
        }
        if trimmed_start.starts_with("test ") && line.trim_end().ends_with("... ok") {
            continue;
        }

        let is_warning = trimmed_start.starts_with("warning:") || line.contains("warning: ");
        if is_warning {
            if warning_count < 5 {
                warning_count += 1;
                result.push(line);
            } else {
                omitted_warnings += 1;
            }
            continue;
        }

        result.push(line);
    }

    let mut output = result.join("\n");
    if omitted_warnings > 0 {
        output.push_str(&format!("\n[{} more warnings omitted]", omitted_warnings));
    }

    output
}

fn filter_npm_output(raw: &str, _threshold: usize) -> String {
    let mut result = Vec::new();
    let mut deprecated_count = 0;

    for line in raw.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.contains("warn deprecated") || lower.contains("warning deprecated") {
            if deprecated_count < 3 {
                deprecated_count += 1;
                result.push(line);
            }
            continue;
        }

        if lower.contains("npm warn") || lower.contains("npm notice") {
            continue;
        }

        if trimmed.starts_with('✓') || trimmed.starts_with("PASS") {
            continue;
        }

        if trimmed.contains("⠋") || trimmed.contains("⠙") || trimmed.contains("⠹") || trimmed.contains("⠸") || trimmed.contains("⠼") || trimmed.contains("⠴") || trimmed.contains("⠦") || trimmed.contains("⠧") || trimmed.contains("⠇") || trimmed.contains("⠏") {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') && (trimmed.contains('=') || trimmed.contains('.')) && trimmed.len() > 10 {
            continue;
        }

        result.push(line);
    }

    result.join("\n")
}

fn filter_git_output(raw: &str, _threshold: usize) -> String {
    let mut result = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Enumerating objects:")
            || trimmed.starts_with("Counting objects:")
            || trimmed.starts_with("Compressing objects:")
            || trimmed.starts_with("remote: Resolving deltas:")
            || trimmed.starts_with("Writing objects:")
            || trimmed.starts_with("remote: Counting objects:")
            || trimmed.starts_with("remote: Compressing objects:")
        {
            continue;
        }
        result.push(line);
    }
    result.join("\n")
}

fn filter_python_output(raw: &str, _threshold: usize) -> String {
    let mut result = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Collecting ")
            || trimmed.starts_with("Downloading ")
            || trimmed.starts_with("Installing collected packages")
            || trimmed.starts_with("Requirement already satisfied:")
        {
            continue;
        }

        if is_python_progress_noise(trimmed) {
            continue;
        }

        result.push(line);
    }
    result.join("\n")
}

fn is_python_progress_noise(trimmed: &str) -> bool {
    let lower = trimmed.to_lowercase();
    if lower.contains("failed") || lower.contains("error") || lower.contains("traceback") {
        return false;
    }
    if trimmed.ends_with("PASSED") || lower.contains("passed [") || lower.contains("passed  [") {
        return true;
    }
    if trimmed.contains('.') && trimmed.contains('%') && (trimmed.contains("test_") || trimmed.contains(".py")) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_cargo_output_compiling_progress() {
        let input = "   Compiling libc v0.2.150\n   Compiling rand v0.8.5\nwarning: unused variable: `x`\n   Compiling myapp v0.1.0\n    Finished dev [unoptimized + debuginfo] target(s) in 2.0s";
        let output = filter_cargo_output(input, 1000);
        assert!(!output.contains("Compiling libc"));
        assert!(!output.contains("Compiling rand"));
        assert!(output.contains("warning: unused variable: `x`"));
        assert!(output.contains("Finished dev"));
    }

    #[test]
    fn test_filter_cargo_output_error() {
        let input = "error[E0308]: mismatched types\n  --> src/main.rs:10:9\n   |\n10 |     let x: String = 123;\n   |                     ^^^ expected struct `String`, found integer";
        let output = filter_cargo_output(input, 1000);
        assert!(output.contains("error[E0308]"));
        assert!(output.contains("mismatched types"));
        assert!(output.contains("expected struct `String`"));
    }

    #[test]
    fn test_filter_cargo_output_passing_test() {
        let input = "running 3 tests\ntest tests::test_one ... ok\ntest tests::test_two ... ok\ntest tests::test_three ... FAILED\n\ntest result: FAILED. 2 passed; 1 failed; 0 ignored;";
        let output = filter_cargo_output(input, 1000);
        assert!(!output.contains("test_one ... ok"));
        assert!(!output.contains("test_two ... ok"));
        assert!(output.contains("test_three ... FAILED"));
        assert!(output.contains("test result: FAILED"));
    }

    #[test]
    fn test_filter_cargo_output_warning_omission() {
        let input = "warning: w1\nwarning: w2\nwarning: w3\nwarning: w4\nwarning: w5\nwarning: w6\nwarning: w7";
        let output = filter_cargo_output(input, 1000);
        assert!(output.contains("warning: w1"));
        assert!(output.contains("warning: w5"));
        assert!(!output.contains("warning: w6"));
        assert!(output.contains("2 more warnings omitted"));
    }

    #[test]
    fn test_filter_npm_output_warnings() {
        let input = "npm WARN deprecated request@2.88.2: request has been deprecated\nnpm notice Cleaned up 12 packages\nadded 1 package in 0.5s";
        let output = filter_npm_output(input, 1000);
        assert!(!output.contains("npm notice"));
        assert!(output.contains("request has been deprecated"));
        assert!(output.contains("added 1 package"));
    }

    #[test]
    fn test_filter_npm_output_jest_passing() {
        let input = "PASS tests/index.test.js\n ✓ test one\n ✕ test two\nFAIL tests/auth.test.js";
        let output = filter_npm_output(input, 1000);
        assert!(!output.contains("PASS tests/index.test.js"));
        assert!(!output.contains("✓ test one"));
        assert!(output.contains("✕ test two"));
        assert!(output.contains("FAIL tests/auth.test.js"));
    }

    #[test]
    fn test_filter_git_output_progress() {
        let input = "Enumerating objects: 5, done.\nCounting objects: 100% (5/5), done.\nCompressing objects: 100% (3/3), done.\nWriting objects: 100% (5/5), 1.2 KiB | 1.2 MiB/s, done.\nTo github.com:user/repo.git\n * [new branch]      main -> main";
        let output = filter_git_output(input, 1000);
        assert!(!output.contains("Enumerating objects"));
        assert!(!output.contains("Counting objects"));
        assert!(!output.contains("Compressing objects"));
        assert!(output.contains("To github.com:user/repo.git"));
        assert!(output.contains("[new branch]"));
    }

    #[test]
    fn test_filter_git_output_conflict() {
        let input = "Auto-merging src/main.rs\nCONFLICT (content): Merge conflict in src/main.rs\nAutomatic merge failed; fix conflicts and then commit the result.";
        let output = filter_git_output(input, 1000);
        assert!(output.contains("CONFLICT"));
        assert!(output.contains("Automatic merge failed"));
    }

    #[test]
    fn test_filter_python_output_traceback() {
        let input = "Traceback (most recent call last):\n  File \"src/main.py\", line 10, in <module>\n    run()\n  File \"src/main.py\", line 5, in run\n    raise ValueError(\"invalid value\")\nValueError: invalid value";
        let output = filter_python_output(input, 1000);
        assert!(output.contains("Traceback"));
        assert!(output.contains("ValueError: invalid value"));
    }

    #[test]
    fn test_filter_python_output_pytest_passed() {
        let input = "tests/test_cli.py . [ 50%]\ntests/test_server.py FAILED [100%]\n=== 1 passed, 1 failed ===";
        let output = filter_python_output(input, 1000);
        assert!(!output.contains("tests/test_cli.py . [ 50%]"));
        assert!(output.contains("tests/test_server.py FAILED [100%]"));
        assert!(output.contains("=== 1 passed, 1 failed ==="));
    }

    #[test]
    fn test_filter_command_output_fallback() {
        let input = "line1\nline1\nline1\nline2\nline2\nline3";
        let output = filter_command_output("ls -la", input, 1000);
        // Should fallback to compress_logs (deduplication)
        assert!(output.contains("line1 [repeated 3 times]"));
    }

    #[test]
    fn test_filter_token_savings_comparison() {
        let input = "   Compiling libc v0.2.150\n   Compiling rand v0.8.5\nrunning 3 tests\ntest tests::test_one ... ok\ntest tests::test_two ... ok\ntest result: ok. 2 passed; 0 failed;";
        let filtered = filter_cargo_output(input, 1000);
        let generic = crate::compression::logs::compress_logs(input, 1000);
        
        let filtered_tokens = crate::intelligence::tokens::estimate_tokens(&filtered);
        let generic_tokens = crate::intelligence::tokens::estimate_tokens(&generic);
        
        // Filtered cargo output should be much smaller
        assert!(filtered_tokens < generic_tokens);
    }
}

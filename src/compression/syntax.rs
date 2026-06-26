/// Lightweight syntax-aware signature extraction for code files.
/// Extracts class, struct, function, and method signatures while truncating bodies.

pub fn extract_signatures(input: &str, extension: &str) -> String {
    let ext = extension.to_lowercase();
    if ext == "py" || ext == "python" {
        extract_python_signatures(input)
    } else {
        // Rust, JavaScript, TypeScript, Go, C++, Java, etc. (brace-based languages)
        extract_brace_signatures(input)
    }
}

fn extract_python_signatures(input: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = input.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if trimmed.starts_with("def ") || trimmed.starts_with("class ") || trimmed.starts_with("@") {
            // Find leading indentation count
            let indent = line.len() - trimmed.len();
            result.push_str(line);
            result.push('\n');

            // Skip block lines that are indented deeper
            i += 1;
            while i < lines.len() {
                let next_line = lines[i];
                let next_trimmed = next_line.trim_start();
                if next_trimmed.is_empty() {
                    i += 1;
                    continue;
                }
                let next_indent = next_line.len() - next_trimmed.len();
                if next_indent <= indent {
                    break;
                }
                // Keep nested signatures (methods inside class)
                if next_trimmed.starts_with("def ") || next_trimmed.starts_with("class ") || next_trimmed.starts_with("@") {
                    result.push_str(next_line);
                    result.push('\n');
                }
                i += 1;
            }
            continue;
        } else if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            result.push_str(line);
            result.push('\n');
        }
        i += 1;
    }
    result.trim().to_string()
}

fn extract_brace_signatures(input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum BlockType {
        Data,      // struct, enum
        Namespace, // impl, class, interface, trait
        Func,      // fn, function
        Other,
    }

    let mut block_stack: Vec<BlockType> = Vec::new();
    let mut pending_block: Option<BlockType> = None;
    let mut last_word = String::new();

    while i < chars.len() {
        let c = chars[i];

        // 1. Handle line comments
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // 2. Handle block comments
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        // 3. Handle double quote strings
        if c == '"' {
            result.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() {
                result.push('"');
                i += 1;
            }
            continue;
        }

        // 4. Handle single quote strings
        if c == '\'' {
            result.push('\'');
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() {
                result.push('\'');
                i += 1;
            }
            continue;
        }

        // 5. Track keywords and words
        if c.is_alphanumeric() || c == '_' {
            last_word.push(c);
        } else {
            if !last_word.is_empty() {
                match last_word.as_str() {
                    "fn" | "function" => pending_block = Some(BlockType::Func),
                    "struct" | "enum" => pending_block = Some(BlockType::Data),
                    "impl" | "class" | "interface" | "trait" => pending_block = Some(BlockType::Namespace),
                    _ => {}
                }
                last_word.clear();
            }
            
            if c == ';' {
                pending_block = None;
            }
        }

        // 6. Handle braces
        if c == '{' {
            let b_type = pending_block.take().unwrap_or(BlockType::Other);
            block_stack.push(b_type);

            if b_type == BlockType::Func {
                result.push_str("{ ... }");

                // Skip everything until we pop this Func block
                let mut temp_stack = vec![b_type];
                i += 1;
                while i < chars.len() && !temp_stack.is_empty() {
                    let tc = chars[i];
                    if tc == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                        i += 2;
                        while i < chars.len() && chars[i] != '\n' { i += 1; }
                        continue;
                    }
                    if tc == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
                        i += 2;
                        while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
                        i += 2;
                        continue;
                    }
                    if tc == '"' {
                        i += 1;
                        while i < chars.len() && chars[i] != '"' {
                            if chars[i] == '\\' && i + 1 < chars.len() { i += 2; } else { i += 1; }
                        }
                        i += 1;
                        continue;
                    }
                    if tc == '\'' {
                        i += 1;
                        while i < chars.len() && chars[i] != '\'' {
                            if chars[i] == '\\' && i + 1 < chars.len() { i += 2; } else { i += 1; }
                        }
                        i += 1;
                        continue;
                    }

                    if tc == '{' {
                        temp_stack.push(BlockType::Other);
                    } else if tc == '}' {
                        temp_stack.pop();
                    }
                    i += 1;
                }
                block_stack.pop();
                continue;
            } else {
                result.push('{');
            }
        } else if c == '}' {
            block_stack.pop();
            result.push('}');
        } else {
            result.push(c);
        }

        i += 1;
    }

    // Collapse blank lines
    let re_blank = regex::Regex::new(r"\n\s*\n").unwrap();
    re_blank.replace_all(&result, "\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_python_signatures() {
        let py_code = r#"
import os

@decorator
class MyClass:
    """Class docstring."""
    def __init__(self):
        self.x = 1

    def compute(self, val):
        result = val * 2
        return result

def global_func():
    print("hello")
"#;
        let expected = "import os\n@decorator\nclass MyClass:\n    def __init__(self):\n    def compute(self, val):\ndef global_func():";
        assert_eq!(extract_signatures(py_code, "py"), expected);
    }

    #[test]
    fn test_extract_rust_signatures() {
        let rust_code = r#"
use std::sync::Arc;

pub struct Foo {
    pub x: i32,
}

impl Foo {
    pub fn new(x: i32) -> Self {
        let y = x + 1;
        Self { x: y }
    }

    pub fn get_x(&self) -> i32 {
        self.x
    }
}
"#;
        let expected = "use std::sync::Arc;\npub struct Foo {\n    pub x: i32,\n}\nimpl Foo {\n    pub fn new(x: i32) -> Self { ... }\n    pub fn get_x(&self) -> i32 { ... }\n}";
        assert_eq!(extract_signatures(rust_code, "rs"), expected);
    }
}

/// Code comment stripping and blank line collapsing.

use regex::Regex;
use std::sync::LazyLock;

static RE_BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static RE_HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
static RE_LINE_COMMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:\s+|^)(?://|#|--).*$").unwrap()
});
static RE_BLANK_LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());

pub fn compress_code(raw_code: &str) -> String {
    let no_blocks = RE_BLOCK_COMMENT.replace_all(raw_code, "");
    let no_html = RE_HTML_COMMENT.replace_all(&no_blocks, "");
    let no_comments = RE_LINE_COMMENTS.replace_all(&no_html, "");
    let collapsed = RE_BLANK_LINES.replace_all(&no_comments, "\n");
    collapsed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

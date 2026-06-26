/// Estimates token count using the cl100k_base heuristic:
/// ~4 characters per token for English text, ~2 characters per token for code
pub fn estimate_tokens(text: &str) -> usize {
    let char_count = text.chars().count();
    (char_count + 3) / 4 // ceil division
}

/// More precise estimate using whitespace + special char splitting
pub fn estimate_tokens_precise(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
            if !ch.is_whitespace() {
                count += 1; // punctuation/special char is counted as a separate token
            }
        }
    }
    // Ensure we return at least 1 if text is not empty
    if count == 0 && !text.is_empty() {
        1
    } else {
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn test_estimate_tokens_precise() {
        assert_eq!(estimate_tokens_precise(""), 0);
        assert_eq!(estimate_tokens_precise("hello"), 1);
        assert_eq!(estimate_tokens_precise("hello world"), 2);
        assert_eq!(estimate_tokens_precise("fn main() {"), 5); // fn, main, (, ), {
    }
}

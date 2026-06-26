/// Helper functions for UTF-8 safe slicing.

pub fn safe_truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[allow(dead_code)]
pub fn safe_tail(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match s.char_indices().rev().nth(max_chars - 1) {
        Some((idx, _)) => &s[idx..],
        None => s,
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
}

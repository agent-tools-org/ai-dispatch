// UTF-8-safe truncation helpers for agent event detail strings.
// Exports truncate_text for CLI adapters that need bounded, single-line text.
// Depends only on the Rust standard library.

const ELLIPSIS: &str = "...";

pub(crate) fn truncate_text(value: &str, max: usize) -> String {
    let value = value.replace('\n', " ");
    if value.len() <= max {
        return value;
    }

    let end = value.floor_char_boundary(max.saturating_sub(ELLIPSIS.len()));
    format!("{}{}", &value[..end], ELLIPSIS)
}

#[cfg(test)]
mod tests {
    use super::truncate_text;

    #[test]
    fn truncates_chinese_at_a_safe_boundary() {
        assert_eq!(truncate_text("ab你好cd", 7), "ab...");
    }

    #[test]
    fn truncates_curly_quotes_at_a_safe_boundary() {
        assert_eq!(truncate_text("ab‘cd’ef", 7), "ab...");
    }

    #[test]
    fn truncates_emoji_at_a_safe_boundary() {
        assert_eq!(truncate_text("ab😀cd", 7), "ab...");
    }
}

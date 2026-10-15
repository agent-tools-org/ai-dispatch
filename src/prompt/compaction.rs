// Knowledge compaction helpers.
// Exports: compact_to_budget.
// Deps: crate::prompt::templates.

/// Truncate text to fit within a token budget, preserving structure.
/// Keeps the first `budget` tokens worth of content, adding a truncation marker.
pub fn compact_to_budget(text: &str, max_tokens: usize) -> String {
    let estimated = crate::prompt::templates::estimate_tokens(text);
    if estimated == 0 || estimated <= max_tokens {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let ratio = (max_tokens.max(1) as f64) / (estimated as f64);
    let keep_lines = ((lines.len() as f64) * ratio).ceil() as usize;
    let keep_lines = keep_lines.clamp(1, lines.len());
    let kept = lines[..keep_lines].join("\n");
    format!("{kept}\n\n[... truncated — {estimated} tokens compressed to ~{max_tokens} ...]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_to_budget_leaves_short_text() {
        let text = "short text";
        assert_eq!(compact_to_budget(text, 10), text);
    }

    #[test]
    fn compact_to_budget_truncates_and_marks() {
        let text = "line1\nline2\nline3\nline4";
        let compacted = compact_to_budget(text, 1);
        assert!(compacted.starts_with("line1"));
        assert!(compacted.contains("[... truncated"));
        assert!(!compacted.contains("line4"));
    }
}

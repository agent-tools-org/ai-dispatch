// UTF-8-safe truncation helpers for agent event detail strings.
// Exports truncate_text and capped_detail for CLI adapters that need bounded,
// single-line text with the untruncated original preserved in event metadata.
// Depends on serde_json for the metadata payload.

const ELLIPSIS: &str = "...";

/// Display cap for event detail strings stored in the DB.
pub(crate) const EVENT_DETAIL_MAX: usize = 80;

pub(crate) fn truncate_text(value: &str, max: usize) -> String {
    let value = value.replace('\n', " ");
    if value.len() <= max {
        return value;
    }

    let end = value.floor_char_boundary(max.saturating_sub(ELLIPSIS.len()));
    format!("{}{}", &value[..end], ELLIPSIS)
}

/// Cap `text` at EVENT_DETAIL_MAX; when it exceeds the cap, preserve the
/// untruncated original under `"full"` in the event metadata.
pub(crate) fn capped_detail(text: &str) -> (String, Option<serde_json::Value>) {
    capped_detail_with(text, None)
}

/// Like `capped_detail`, merging `"full"` into pre-existing metadata.
pub(crate) fn capped_detail_with(
    text: &str,
    metadata: Option<serde_json::Value>,
) -> (String, Option<serde_json::Value>) {
    let detail = truncate_text(text, EVENT_DETAIL_MAX);
    if text.len() <= EVENT_DETAIL_MAX {
        return (detail, metadata);
    }
    let mut metadata = metadata.unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("full".to_string(), serde_json::json!(text));
    }
    (detail, Some(metadata))
}

#[cfg(test)]
mod tests {
    use super::{capped_detail, capped_detail_with, truncate_text, EVENT_DETAIL_MAX};

    #[test]
    fn capped_detail_below_cap_has_no_full_metadata() {
        let (detail, metadata) = capped_detail("short detail");
        assert_eq!(detail, "short detail");
        assert!(metadata.is_none());
    }

    #[test]
    fn capped_detail_above_cap_stores_full_in_metadata() {
        let text = "x".repeat(EVENT_DETAIL_MAX + 20);
        let (detail, metadata) = capped_detail(&text);
        assert_eq!(detail.len(), EVENT_DETAIL_MAX);
        assert!(detail.ends_with("..."));
        let metadata = metadata.expect("full metadata above cap");
        assert_eq!(metadata["full"].as_str(), Some(text.as_str()));
    }

    #[test]
    fn capped_detail_with_merges_into_existing_metadata() {
        let text = "y".repeat(EVENT_DETAIL_MAX + 1);
        let existing = serde_json::json!({ "model": "m1" });
        let (_, metadata) = capped_detail_with(&text, Some(existing));
        let metadata = metadata.expect("metadata present");
        assert_eq!(metadata["model"].as_str(), Some("m1"));
        assert_eq!(metadata["full"].as_str(), Some(text.as_str()));

        let (_, metadata) = capped_detail_with("short", Some(serde_json::json!({ "model": "m1" })));
        let metadata = metadata.expect("existing metadata preserved");
        assert!(metadata.get("full").is_none());
    }

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

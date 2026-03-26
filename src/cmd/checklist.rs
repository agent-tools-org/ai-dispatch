// Checklist helpers for `aid run` prompt injection and file loading.
// Exports: merge_checklist_items(), load_checklist_file(), format_checklist_block().
// Deps: anyhow for file IO context and std::fs for checklist files.

use anyhow::{Context, Result};

pub(crate) fn merge_checklist_items(
    inline_items: Vec<String>,
    checklist_file: Option<&str>,
) -> Result<Vec<String>> {
    let mut merged = inline_items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if let Some(path) = checklist_file {
        merged.extend(load_checklist_file(path)?);
    }
    Ok(merged)
}

pub(crate) fn load_checklist_file(path: &str) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read checklist file: {path}"))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToString::to_string)
        .collect())
}

pub(crate) fn format_checklist_block(items: &[String]) -> Option<String> {
    let items = items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }

    let mut lines = vec![
        "<aid-checklist>".to_string(),
        "MANDATORY CHECKLIST — You MUST explicitly address EVERY item below.".to_string(),
        "For each item, state CONFIRMED (with evidence) or REJECTED (with reasoning).".to_string(),
        "Do NOT skip any item. Missing responses will trigger an automatic retry.".to_string(),
        String::new(),
    ];
    lines.extend(
        items
            .iter()
            .enumerate()
            .map(|(idx, item)| format!("[ ] {}. {}", idx + 1, item)),
    );
    lines.push("</aid-checklist>".to_string());
    Some(lines.join("\n"))
}

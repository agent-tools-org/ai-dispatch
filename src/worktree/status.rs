// The one parser for `git status --porcelain` v1 lines.
// Exports parse_porcelain_line plus the per-consumer readings built on it.
// Deps: snapshot entry types only; never unquotes beyond status_line_paths.

use super::snapshot::{WorktreeStatusEntry, WorktreeStatusKind};

/// One porcelain line, borrowed and undecoded: git's quoting is left in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatusEntry<'a> {
    /// XY status bytes; porcelain v1 keeps them ASCII.
    pub x: char,
    pub y: char,
    /// The line starts with exactly `"?? "`.
    pub untracked: bool,
    /// Destination of a naive first `" -> "` split; all of `rest` for untracked lines.
    pub path: &'a str,
    /// Raw text after `"XY "`, unsplit.
    pub rest: &'a str,
}

/// `None` for a line too short to hold `"XY "`.
pub(crate) fn parse_porcelain_line(line: &str) -> Option<StatusEntry<'_>> {
    let rest = line.get(3..)?;
    let bytes = line.as_bytes();
    let (x, y) = (char::from(*bytes.first()?), char::from(*bytes.get(1)?));
    let untracked = line.starts_with("?? ");
    let path = match rest.split_once(" -> ") {
        Some((_, to)) if !untracked => to,
        _ => rest,
    };
    Some(StatusEntry { x, y, untracked, path, rest })
}

/// Rescue's reading: untracked files, and anything with an `M` in either column.
/// A rename keeps its whole `old -> new` text as the path.
pub(crate) fn parse_status_entry(line: &str) -> Option<WorktreeStatusEntry> {
    let entry = parse_porcelain_line(line)?;
    let kind = if entry.untracked {
        WorktreeStatusKind::Untracked
    } else if line.len() >= 4 && (entry.x == 'M' || entry.y == 'M') {
        WorktreeStatusKind::Modified
    } else {
        return None;
    };
    Some(WorktreeStatusEntry { path: entry.rest.to_string(), kind })
}

/// Every path a porcelain line names: one, or two for a rename. Untracked lines are
/// split too, and each side loses a wrapping pair of quotes.
pub(crate) fn status_line_paths(line: &str) -> Vec<String> {
    let Some(entry) = parse_porcelain_line(line) else {
        return Vec::new();
    };
    match entry.rest.split_once(" -> ") {
        Some((from, to)) => vec![unquote(from), unquote(to)],
        None => vec![unquote(entry.rest)],
    }
}

/// Git quotes paths that need escaping (`?? "odd name.md"`). Strip the wrapper so the
/// name is judged, not the quote character.
fn unquote(path: &str) -> String {
    path.strip_prefix('"').and_then(|p| p.strip_suffix('"')).unwrap_or(path).to_string()
}

/// The baseline's reading: the raw destination path, or `None` under four bytes.
pub(crate) fn extract_baseline_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    parse_porcelain_line(line).map(|entry| entry.path.to_string())
}

/// Prune's reading: the trimmed text after `"XY "`, or the whole trimmed line when
/// nothing follows it.
pub(crate) fn status_line_path(line: &str) -> &str {
    match parse_porcelain_line(line) {
        Some(entry) if line.len() > 3 => entry.rest.trim(),
        _ => line.trim(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeStatusSummary {
    pub(crate) modified: usize,
    pub(crate) staged: usize,
    pub(crate) untracked: usize,
}

/// Counts by status column; a short line is read by whatever columns it has.
pub(crate) fn summarize_status(lines: &[String]) -> WorktreeStatusSummary {
    let mut summary = WorktreeStatusSummary { modified: 0, staged: 0, untracked: 0 };
    for line in lines {
        if parse_porcelain_line(line).is_some_and(|entry| entry.untracked) {
            summary.untracked += 1;
            continue;
        }
        let mut chars = line.chars();
        let (index, worktree) = (chars.next().unwrap_or(' '), chars.next().unwrap_or(' '));
        if index != ' ' {
            summary.staged += 1;
        }
        if worktree != ' ' {
            summary.modified += 1;
        }
    }
    summary
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;

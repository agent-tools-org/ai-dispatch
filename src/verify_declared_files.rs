// Declared-file verification for task prompts that promise new files.
// Exports check_task_declared_files and pure prompt/diff helpers for tests.
// Deps: anyhow, git CLI, and task metadata.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::types::Task;

const CREATE_COLON_MARKERS: &[&str] = &[
    "create a new files:",
    "create a new file:",
    "create new files:",
    "create new file:",
    "create a files:",
    "create a file:",
    "create files:",
    "create file:",
];

pub(crate) fn check_task_declared_files(
    worktree_path: &Path,
    task: &Task,
) -> Result<Option<String>> {
    let declared = parse_declared_new_files(&task.prompt);
    if declared.is_empty() {
        return Ok(None);
    }
    let added = added_paths_from_git_diff(worktree_path, task.start_sha.as_deref())?;
    Ok(declared_file_failure_message(&task.prompt, &added))
}

fn declared_file_failure_message(prompt: &str, added_paths: &[String]) -> Option<String> {
    let missing = missing_declared_paths(&parse_declared_new_files(prompt), added_paths);
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "Declared new file verification failed: missing added file(s): {}",
        missing.join(", ")
    ))
}

fn parse_declared_new_files(prompt: &str) -> Vec<String> {
    let lines: Vec<&str> = prompt.lines().collect();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if let Some((remainder, collect_following)) = declaration_remainder(lines[index]) {
            if remainder.trim().is_empty() {
                index = collect_following_path_lines(&lines, index + 1, &mut paths);
                continue;
            }
            add_paths_from_segment(remainder, &mut paths);
            if collect_following {
                index = collect_following_path_lines(&lines, index + 1, &mut paths);
                continue;
            }
        }
        index += 1;
    }
    paths
}

fn collect_following_path_lines(lines: &[&str], mut index: usize, paths: &mut Vec<String>) -> usize {
    while index < lines.len() {
        let line = lines[index].trim();
        if line.is_empty() || declaration_remainder(line).is_some() {
            break;
        }
        add_paths_from_segment(line, paths);
        index += 1;
    }
    index
}

fn declaration_remainder(line: &str) -> Option<(&str, bool)> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    for marker in CREATE_COLON_MARKERS {
        if lower.starts_with(marker) {
            return Some((&trimmed[marker.len()..], marker.contains("files:")));
        }
    }
    let at_marker = "create a new file at ";
    if lower.starts_with(at_marker) {
        return Some((&trimmed[at_marker.len()..], false));
    }
    let new_file_marker = "new file:";
    if lower.starts_with(new_file_marker) {
        return Some((&trimmed[new_file_marker.len()..], false));
    }
    None
}

fn add_paths_from_segment(segment: &str, paths: &mut Vec<String>) {
    for item in segment.split(',') {
        let Some(path) = normalize_declared_path(item) else {
            continue;
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
}

fn normalize_declared_path(raw: &str) -> Option<String> {
    let item = trim_list_marker(raw).trim();
    let item = extract_quoted(item).unwrap_or(item);
    let item = item
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['"', '\'', '`'])
        .trim_end_matches(['.', ';', ':', '!', '?', ')', ']', '}']);
    let item = item.trim_start_matches("./");
    if is_probable_path(item) {
        Some(item.to_string())
    } else {
        None
    }
}

fn trim_list_marker(raw: &str) -> &str {
    let item = raw.trim_start();
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = item.strip_prefix(prefix) {
            return rest;
        }
    }
    item
}

fn extract_quoted(item: &str) -> Option<&str> {
    let quote = item.chars().next()?;
    if !matches!(quote, '"' | '\'' | '`') {
        return None;
    }
    let start = quote.len_utf8();
    let end = item[start..].find(quote)?;
    Some(&item[start..start + end])
}

fn is_probable_path(path: &str) -> bool {
    if path.is_empty() || path.chars().any(char::is_whitespace) {
        return false;
    }
    path.contains('/')
        || path.contains('\\')
        || path.contains('.')
        || matches!(path, "Makefile" | "Dockerfile" | "LICENSE" | "README")
}

fn added_paths_from_git_diff(worktree_path: &Path, start_sha: Option<&str>) -> Result<Vec<String>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(worktree_path)
        .args(["diff", "--name-status"]);
    match start_sha.filter(|sha| !sha.trim().is_empty()) {
        Some(sha) => {
            cmd.arg(format!("{sha}..HEAD"));
        }
        None => {
            cmd.arg("HEAD^..HEAD");
        }
    }
    cmd.args(["--", "."]);
    let output = cmd.output().context("failed to run git diff --name-status")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff --name-status failed: {}", stderr.trim());
    }
    Ok(parse_added_paths_from_name_status(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_added_paths_from_name_status(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|line| {
            let (status, path) = line.split_once('\t')?;
            if status == "A" {
                Some(path.trim().trim_start_matches("./").to_string())
            } else {
                None
            }
        })
        .collect()
}

fn missing_declared_paths(declared_paths: &[String], added_paths: &[String]) -> Vec<String> {
    declared_paths
        .iter()
        .filter(|path| !added_paths.iter().any(|added| added == *path))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_create_new_file() {
        let paths = parse_declared_new_files("Create a NEW file: path/to/x.rs");

        assert_eq!(paths, vec!["path/to/x.rs"]);
    }

    #[test]
    fn parses_create_files_list() {
        let paths = parse_declared_new_files("Create files: a.rs, b.rs");

        assert_eq!(paths, vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn strips_backticks_and_quotes() {
        let paths = parse_declared_new_files("Create files: `a.rs`, \"b.rs\", 'c.rs'.");

        assert_eq!(paths, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn prompt_without_declaration_returns_success() {
        let failure = declared_file_failure_message("Update README.md only", &[]);

        assert!(failure.is_none());
    }

    #[test]
    fn parses_newline_separated_declarations() {
        let paths = parse_declared_new_files("Create files:\n- a.rs\n- b.rs\n\nThen test it");

        assert_eq!(paths, vec!["a.rs", "b.rs"]);

        let inline_start = parse_declared_new_files("Create files: c.rs\nd.rs\n\nThen test it");
        assert_eq!(inline_start, vec!["c.rs", "d.rs"]);
    }

    #[test]
    fn detects_missing_and_present_added_files() {
        let added = parse_added_paths_from_name_status("A\tlib/foo.rs\nM\tREADME.md\nA\tlib/bar.rs\n");
        let declared = vec!["lib/foo.rs".to_string(), "lib/missing.rs".to_string()];

        assert_eq!(missing_declared_paths(&declared, &added), vec!["lib/missing.rs"]);
        assert!(missing_declared_paths(&["lib/bar.rs".to_string()], &added).is_empty());
    }
}

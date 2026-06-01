// Dirty baseline parsing shared by rescue and final assertion logic.
// Exports porcelain status path extraction and baseline membership helpers.
// Deps: std collections only.

use std::collections::HashSet;

pub(crate) fn extract_baseline_paths(baseline: &[String]) -> HashSet<String> {
    baseline
        .iter()
        .filter_map(|line| extract_baseline_path(line))
        .collect()
}

pub(crate) fn extract_baseline_path(line: &str) -> Option<String> {
    if line.is_empty() || line.len() < 4 {
        return None;
    }
    if let Some(path) = line.strip_prefix("?? ") {
        return Some(path.to_string());
    }
    let path = &line[3..];
    if let Some((_, renamed_path)) = path.split_once(" -> ") {
        return Some(renamed_path.to_string());
    }
    Some(path.to_string())
}

pub(crate) fn baseline_contains(baseline: &HashSet<String>, path: &str) -> bool {
    baseline.contains(path)
}

#[cfg(test)]
mod tests {
    use super::{baseline_contains, extract_baseline_path, extract_baseline_paths};

    #[test]
    fn extracts_untracked_modified_and_renamed_paths() {
        assert_eq!(extract_baseline_path("?? src/new.rs").as_deref(), Some("src/new.rs"));
        assert_eq!(extract_baseline_path(" M src/lib.rs").as_deref(), Some("src/lib.rs"));
        assert_eq!(extract_baseline_path("R  src/a.rs -> src/b.rs").as_deref(), Some("src/b.rs"));
    }

    #[test]
    fn ignores_malformed_status_lines() {
        assert_eq!(extract_baseline_path(""), None);
        assert_eq!(extract_baseline_path(" M"), None);
    }

    #[test]
    fn baseline_membership_uses_extracted_paths() {
        let baseline = extract_baseline_paths(&[
            "?? src/new.rs".to_string(),
            "R  src/a.rs -> src/b.rs".to_string(),
        ]);

        assert!(baseline_contains(&baseline, "src/new.rs"));
        assert!(baseline_contains(&baseline, "src/b.rs"));
        assert!(!baseline_contains(&baseline, "src/a.rs"));
    }
}

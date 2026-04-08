// Batch overlap analysis for pre-dispatch conflict warnings.
// Exports: analyze_file_overlap(), FileOverlap.
// Deps: crate::batch
use crate::batch::{self, BatchDefaults, BatchTask};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOverlap {
    pub file: String,
    pub task_ids: Vec<String>,
    pub severity: OverlapSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlapSeverity { Warning, Error }

pub(super) fn analyze_file_overlap(tasks: &[BatchTask], defaults: &BatchDefaults) -> Vec<FileOverlap> {
    let dependencies = batch::dependency_indices(tasks).unwrap_or_else(|_| vec![Vec::new(); tasks.len()]);
    let reachability = build_reachability(&dependencies);
    let mut file_map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        for file in task_files(task, defaults) {
            file_map.entry(file).or_default().push(task_idx);
        }
    }
    file_map
        .into_iter()
        .filter_map(|(file, task_indices)| build_overlap(file, &task_indices, tasks, &reachability))
        .collect()
}

fn build_overlap(file: String, task_indices: &[usize], tasks: &[BatchTask], reachability: &[Vec<bool>]) -> Option<FileOverlap> {
    let mut overlap_tasks = BTreeSet::new();
    let mut severity = OverlapSeverity::Warning;
    for (position, left_idx) in task_indices.iter().enumerate() {
        for right_idx in &task_indices[position + 1..] {
            if reachability[*left_idx][*right_idx] || reachability[*right_idx][*left_idx] {
                continue;
            }
            if tasks[*left_idx].output.as_deref() == Some(file.as_str())
                || tasks[*right_idx].output.as_deref() == Some(file.as_str())
                || tasks[*left_idx].result_file.as_deref() == Some(file.as_str())
                || tasks[*right_idx].result_file.as_deref() == Some(file.as_str())
            {
                severity = OverlapSeverity::Error;
            }
            overlap_tasks.insert(task_ref(tasks, *left_idx));
            overlap_tasks.insert(task_ref(tasks, *right_idx));
        }
    }
    if overlap_tasks.len() < 2 {
        return None;
    }
    Some(FileOverlap {
        file,
        task_ids: overlap_tasks.into_iter().collect(),
        severity,
    })
}

fn build_reachability(dependencies: &[Vec<usize>]) -> Vec<Vec<bool>> {
    let mut memo = HashMap::new();
    (0..dependencies.len())
        .map(|task_idx| {
            let reachable = reachable_from(task_idx, dependencies, &mut memo);
            let mut row = vec![false; dependencies.len()];
            for dep_idx in reachable {
                row[dep_idx] = true;
            }
            row
        })
        .collect()
}

fn reachable_from(
    task_idx: usize,
    dependencies: &[Vec<usize>],
    memo: &mut HashMap<usize, HashSet<usize>>,
) -> HashSet<usize> {
    if let Some(reachable) = memo.get(&task_idx) {
        return reachable.clone();
    }
    let mut reachable = HashSet::new();
    for dep_idx in &dependencies[task_idx] {
        reachable.insert(*dep_idx);
        reachable.extend(reachable_from(*dep_idx, dependencies, memo));
    }
    memo.insert(task_idx, reachable.clone());
    reachable
}

fn task_files(task: &BatchTask, defaults: &BatchDefaults) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    // read_only tasks don't modify files — skip context to avoid false overlap warnings (GH#60)
    if !task.read_only {
        if let Some(context) = task.context.as_ref().or(defaults.context.as_ref()) {
            for file in context {
                if is_file_path(file) {
                    files.insert(trim_candidate(file).to_string());
                }
            }
        }
    }
    if let Some(ref output) = task.output {
        files.insert(output.clone());
    }
    if let Some(ref result_file) = task.result_file {
        files.insert(result_file.clone());
    }
    for file in extract_prompt_paths(&task.prompt) {
        files.insert(file);
    }
    files
}

fn extract_prompt_paths(prompt: &str) -> BTreeSet<String> {
    prompt
        .split_whitespace()
        .filter_map(|token| is_file_path(trim_candidate(token)).then(|| trim_candidate(token).to_string()))
        .collect()
}

fn is_file_path(candidate: &str) -> bool {
    if candidate.is_empty()
        || candidate.starts_with("--")
        || candidate.starts_with("http://")
        || candidate.starts_with("https://")
        || candidate.contains("://")
    {
        return false;
    }
    let Some(file_name) = candidate.rsplit('/').next() else {
        return false;
    };
    let Some((_, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    if extension.is_empty() || !extension.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }
    candidate.contains('/') || matches!(extension, "rs" | "ts" | "tsx" | "md" | "json" | "toml" | "yaml" | "yml" | "txt" | "csv" | "html" | "css" | "js" | "py" | "sh" | "sql")
}

fn trim_candidate(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        matches!(ch, '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ':' | ';' | '.')
    })
}

fn task_ref(tasks: &[BatchTask], task_idx: usize) -> String {
    tasks[task_idx]
        .name
        .as_ref()
        .or(tasks[task_idx].id.as_ref())
        .cloned()
        .unwrap_or_else(|| format!("#{task_idx}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_task(name: &str, prompt: &str) -> BatchTask {
        BatchTask {
            id: None,
            name: Some(name.to_string()),
            agent: "codex".to_string(),
            team: None,
            prompt: prompt.to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            best_of: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            budget: false,
            env: None,
            env_forward: None,
            on_success: None,
            on_fail: None,
            conditional: false,
        }
    }

    #[test]
    fn analyze_detects_overlapping_context_files() {
        let mut left = stub_task("task-a", "left");
        left.context = Some(vec!["src/types.rs".to_string()]);
        let mut right = stub_task("task-b", "right");
        right.context = Some(vec!["src/types.rs".to_string()]);
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps, vec![FileOverlap {
            file: "src/types.rs".to_string(),
            task_ids: vec!["task-a".to_string(), "task-b".to_string()],
            severity: OverlapSeverity::Warning,
        }]);
    }

    #[test]
    fn analyze_ignores_tasks_with_dependency() {
        let mut parent = stub_task("task-a", "touch src/types.rs");
        parent.context = Some(vec!["src/types.rs".to_string()]);
        let mut child = stub_task("task-b", "touch src/types.rs");
        child.depends_on = Some(vec!["task-a".to_string()]);
        let overlaps = analyze_file_overlap(&[parent, child], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

    #[test]
    fn analyze_extracts_paths_from_prompt() {
        let left = stub_task("task-a", "Update src/types.rs and keep tests green.");
        let right = stub_task("task-b", "Review src/types.rs for shared changes.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "src/types.rs");
        assert_eq!(overlaps[0].task_ids, vec!["task-a".to_string(), "task-b".to_string()]);
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn analyze_skips_context_for_read_only_tasks() {
        let mut writer = stub_task("writer", "edit something");
        writer.context = Some(vec!["src/main.rs".to_string()]);
        let mut reader = stub_task("reader", "review something");
        reader.context = Some(vec!["src/main.rs".to_string()]);
        reader.read_only = true;
        let overlaps = analyze_file_overlap(&[writer, reader], &BatchDefaults::default());
        assert!(overlaps.is_empty(), "read_only task should not trigger overlap warning");
    }

    #[test]
    fn analyze_no_false_positives_on_urls() {
        let left = stub_task("task-a", "See https://example.com/src/types.rs for context.");
        let right = stub_task("task-b", "No files mentioned here.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

    #[test]
    fn analyze_detects_overlapping_output_files() {
        let mut left = stub_task("task-a", "write findings");
        left.output = Some("FINDINGS.md".to_string());
        let mut right = stub_task("task-b", "write findings");
        right.output = Some("FINDINGS.md".to_string());
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "FINDINGS.md");
        assert_eq!(overlaps[0].task_ids, vec!["task-a".to_string(), "task-b".to_string()]);
        assert_eq!(overlaps[0].severity, OverlapSeverity::Error);
    }

    #[test]
    fn analyze_output_overlap_ignored_with_dependency() {
        let mut parent = stub_task("task-a", "write findings");
        parent.output = Some("FINDINGS.md".to_string());
        let mut child = stub_task("task-b", "write findings");
        child.output = Some("FINDINGS.md".to_string());
        child.depends_on = Some(vec!["task-a".to_string()]);
        let overlaps = analyze_file_overlap(&[parent, child], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

    #[test]
    fn analyze_detects_md_paths_in_prompt() {
        let left = stub_task("task-a", "Write findings to FINDINGS.md.");
        let right = stub_task("task-b", "Review FINDINGS.md before merge.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "FINDINGS.md");
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn analyze_detects_json_paths_in_prompt() {
        let left = stub_task("task-a", "Write results.json for the run.");
        let right = stub_task("task-b", "Validate results.json structure.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "results.json");
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn analyze_detects_result_file_overlap() {
        let mut left = stub_task("task-a", "audit left");
        left.result_file = Some("result.md".to_string());
        let mut right = stub_task("task-b", "audit right");
        right.result_file = Some("result.md".to_string());
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "result.md");
        assert_eq!(overlaps[0].severity, OverlapSeverity::Error);
    }

    #[test]
    fn analyze_result_file_overlap_ignored_with_dependency() {
        let mut parent = stub_task("task-a", "audit left");
        parent.result_file = Some("result.md".to_string());
        let mut child = stub_task("task-b", "audit right");
        child.result_file = Some("result.md".to_string());
        child.depends_on = Some(vec!["task-a".to_string()]);
        let overlaps = analyze_file_overlap(&[parent, child], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }
}

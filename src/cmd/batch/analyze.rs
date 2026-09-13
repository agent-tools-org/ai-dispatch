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
#[path = "analyze_tests.rs"]
mod tests;

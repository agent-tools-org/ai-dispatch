// Auto-dedup conflicting output paths for parallel batch tasks.
// Exports: dedup_output_paths(); Deps: crate::batch, super::batch_validate
use super::batch_validate::task_label;
use crate::batch::BatchTask;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
pub(super) fn dedup_output_paths(tasks: &mut [BatchTask], dependencies: &[Vec<usize>]) -> usize {
    let mut outputs = BTreeMap::<String, Vec<usize>>::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        if let Some(output) = task.output.as_ref() {
            outputs.entry(output.clone()).or_default().push(task_idx);
        }
    }
    let mut renamed = 0;
    for (output, task_indices) in outputs {
        if task_indices.len() < 2 || !has_parallel_conflict(&task_indices, dependencies) {
            continue;
        }
        aid_info!("[aid] Output conflict: {} tasks target \"{}\". Auto-suffixed to avoid data loss:", task_indices.len(), output);
        for task_idx in task_indices {
            let new_output = suffixed_path(&output, &task_suffix(&tasks[task_idx], task_idx));
            aid_info!("  {}: {} -> {}", task_label(&tasks[task_idx], task_idx), output, new_output.display());
            tasks[task_idx].output = Some(new_output.to_string_lossy().into_owned());
            renamed += 1;
        }
    }
    renamed
}
fn has_parallel_conflict(task_indices: &[usize], dependencies: &[Vec<usize>]) -> bool {
    task_indices.iter().enumerate().any(|(pos, &left)| task_indices[pos + 1..].iter().any(|&right| {
        !depends_on(left, right, dependencies) && !depends_on(right, left, dependencies)
    }))
}
fn depends_on(task_idx: usize, target_idx: usize, dependencies: &[Vec<usize>]) -> bool {
    let mut stack = dependencies[task_idx].clone();
    while let Some(dep_idx) = stack.pop() {
        if dep_idx == target_idx {
            return true;
        }
        stack.extend(dependencies[dep_idx].iter().copied());
    }
    false
}
fn task_suffix(task: &BatchTask, task_idx: usize) -> String {
    task.name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.replace(['/', '\\'], "-"))
        .unwrap_or_else(|| format!("task-{task_idx}"))
}
fn suffixed_path(output: &str, suffix: &str) -> PathBuf {
    let path = Path::new(output);
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or(output);
    let file_name = path.extension().and_then(|value| value.to_str())
        .map(|extension| format!("{stem}-{suffix}.{extension}"))
        .unwrap_or_else(|| format!("{stem}-{suffix}"));
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn stub_task(name: Option<&str>, output: Option<&str>) -> BatchTask {
        BatchTask {
            id: None,
            name: name.map(str::to_string),
            agent: "codex".to_string(),
            team: None,
            prompt: "prompt".to_string(),
            dir: None,
            output: output.map(str::to_string),
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
            retry: None,
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
    fn dedup_renames_parallel_conflicts() {
        let mut tasks = vec![stub_task(Some("alpha"), Some("findings.md")), stub_task(Some("beta"), Some("findings.md")), stub_task(Some("gamma"), Some("findings.md"))];
        assert_eq!(dedup_output_paths(&mut tasks, &[vec![], vec![], vec![]]), 3);
        assert_eq!(tasks[0].output.as_deref(), Some("findings-alpha.md"));
        assert_eq!(tasks[1].output.as_deref(), Some("findings-beta.md"));
        assert_eq!(tasks[2].output.as_deref(), Some("findings-gamma.md"));
    }
    #[test]
    fn dedup_skips_sequential_tasks() {
        let mut tasks = vec![stub_task(Some("alpha"), Some("findings.md")), stub_task(Some("beta"), Some("findings.md"))];
        assert_eq!(dedup_output_paths(&mut tasks, &[vec![], vec![0]]), 0);
        assert_eq!(tasks[0].output.as_deref(), Some("findings.md"));
        assert_eq!(tasks[1].output.as_deref(), Some("findings.md"));
    }
    #[test]
    fn dedup_preserves_non_conflicting() {
        let mut tasks = vec![stub_task(Some("alpha"), Some("a.md")), stub_task(Some("beta"), Some("b.md"))];
        assert_eq!(dedup_output_paths(&mut tasks, &[vec![], vec![]]), 0);
        assert_eq!(tasks[0].output.as_deref(), Some("a.md"));
        assert_eq!(tasks[1].output.as_deref(), Some("b.md"));
    }
    #[test]
    fn dedup_handles_unnamed_tasks() {
        let mut tasks = vec![stub_task(None, Some("findings.md")), stub_task(None, Some("findings.md"))];
        assert_eq!(dedup_output_paths(&mut tasks, &[vec![], vec![]]), 2);
        assert_eq!(tasks[0].output.as_deref(), Some("findings-task-0.md"));
        assert_eq!(tasks[1].output.as_deref(), Some("findings-task-1.md"));
    }
    #[test]
    fn dedup_handles_path_with_directory() {
        let mut tasks = vec![stub_task(Some("taskA"), Some("reports/out.json")), stub_task(Some("taskB"), Some("reports/out.json"))];
        assert_eq!(dedup_output_paths(&mut tasks, &[vec![], vec![]]), 2);
        assert_eq!(tasks[0].output.as_deref(), Some("reports/out-taskA.json"));
        assert_eq!(tasks[1].output.as_deref(), Some("reports/out-taskB.json"));
    }
}

// Batch task file parser: reads TOML batch configs and validates task DAGs.
// Each batch file declares tasks with agent, prompt, overrides, and dependencies.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const VALID_AGENTS: &[&str] = &["gemini", "codex", "opencode", "cursor"];

#[derive(Debug, Deserialize)]
pub struct BatchConfig {
    #[serde(rename = "task")]
    pub tasks: Vec<BatchTask>,
}

#[derive(Debug, Deserialize)]
pub struct BatchTask {
    pub name: Option<String>,
    pub agent: String,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub group: Option<String>,
    pub verify: Option<String>,
    pub skills: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
}

pub fn parse_batch_file(path: &Path) -> Result<BatchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read batch file: {}", path.display()))?;
    let config: BatchConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    if config.tasks.is_empty() {
        anyhow::bail!("batch file contains no tasks");
    }
    for task in &config.tasks {
        if !VALID_AGENTS.contains(&task.agent.to_lowercase().as_str()) {
            anyhow::bail!("unknown agent: {}", task.agent);
        }
    }
    validate_no_file_overlap(&config.tasks)?;
    validate_dag(&config.tasks)?;
    Ok(config)
}

pub fn validate_no_file_overlap(tasks: &[BatchTask]) -> Result<()> {
    let mut seen: HashSet<&str> = HashSet::new();
    for task in tasks {
        if let Some(ref wt) = task.worktree
            && !seen.insert(wt.as_str())
        {
            anyhow::bail!("duplicate worktree: {}", wt);
        }
    }
    Ok(())
}
pub fn validate_dag(tasks: &[BatchTask]) -> Result<()> {
    let dependencies = dependency_indices(tasks)?;
    let mut states = vec![VisitState::Pending; tasks.len()];
    for task_idx in 0..tasks.len() {
        visit_task(task_idx, tasks, &dependencies, &mut states)?;
    }
    Ok(())
}
pub fn topo_levels(tasks: &[BatchTask]) -> Result<Vec<Vec<usize>>> {
    let dependencies = dependency_indices(tasks)?;
    let mut indegree = vec![0usize; tasks.len()];
    let mut dependents = vec![Vec::new(); tasks.len()];

    for (task_idx, deps) in dependencies.iter().enumerate() {
        indegree[task_idx] = deps.len();
        for &dep_idx in deps {
            dependents[dep_idx].push(task_idx);
        }
    }

    let mut ready: Vec<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(task_idx, &count)| (count == 0).then_some(task_idx))
        .collect();
    let mut levels = Vec::new();
    let mut processed = 0usize;

    while !ready.is_empty() {
        levels.push(ready.clone());
        processed += ready.len();

        let mut next = Vec::new();
        for task_idx in ready {
            for &dependent_idx in &dependents[task_idx] {
                indegree[dependent_idx] -= 1;
                if indegree[dependent_idx] == 0 {
                    next.push(dependent_idx);
                }
            }
        }
        next.sort_unstable();
        ready = next;
    }

    anyhow::ensure!(processed == tasks.len(), "dependency cycle detected");
    Ok(levels)
}
pub(crate) fn dependency_indices(tasks: &[BatchTask]) -> Result<Vec<Vec<usize>>> {
    let name_to_index = task_name_map(tasks)?;
    tasks.iter().enumerate().map(|(task_idx, task)| {
        resolve_dependencies(task_idx, task, &name_to_index)
    }).collect()
}
fn task_name_map(tasks: &[BatchTask]) -> Result<HashMap<&str, usize>> {
    let mut name_to_index = HashMap::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        let Some(name) = task.name.as_deref() else {
            continue;
        };
        let trimmed = name.trim();
        anyhow::ensure!(!trimmed.is_empty(), "task {task_idx} has an empty name");
        if name_to_index.insert(trimmed, task_idx).is_some() {
            anyhow::bail!("duplicate task name: {trimmed}");
        }
    }
    Ok(name_to_index)
}
fn resolve_dependencies(
    task_idx: usize,
    task: &BatchTask,
    name_to_index: &HashMap<&str, usize>,
) -> Result<Vec<usize>> {
    let Some(depends_on) = task.depends_on.as_ref() else {
        return Ok(Vec::new());
    };
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    for dependency_name in depends_on {
        let trimmed = dependency_name.trim();
        anyhow::ensure!(
            !trimmed.is_empty(),
            "task {} has an empty dependency reference",
            task_label(task, task_idx)
        );
        let Some(&dependency_idx) = name_to_index.get(trimmed) else {
            anyhow::bail!(
                "task {} depends on unknown task: {}",
                task_label(task, task_idx),
                trimmed
            );
        };
        if seen.insert(dependency_idx) {
            resolved.push(dependency_idx);
        }
    }
    Ok(resolved)
}
fn task_label(task: &BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState { Pending, Visiting, Visited }
fn visit_task(
    task_idx: usize,
    tasks: &[BatchTask],
    dependencies: &[Vec<usize>],
    states: &mut [VisitState],
) -> Result<()> {
    match states[task_idx] {
        VisitState::Visited => return Ok(()),
        VisitState::Visiting => {
            anyhow::bail!(
                "dependency cycle detected at task {}",
                task_label(&tasks[task_idx], task_idx)
            );
        }
        VisitState::Pending => {}
    }
    states[task_idx] = VisitState::Visiting;
    for &dependency_idx in &dependencies[task_idx] {
        visit_task(dependency_idx, tasks, dependencies, states)?;
    }
    states[task_idx] = VisitState::Visited;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn make_task(name: Option<&str>, depends_on: &[&str]) -> BatchTask {
        BatchTask {
            name: name.map(str::to_string),
            agent: "codex".to_string(),
            prompt: "prompt".to_string(),
            dir: None,
            output: None,
            model: None,
            worktree: None,
            group: None,
            verify: None,
            skills: None,
            depends_on: (!depends_on.is_empty())
                .then(|| depends_on.iter().map(|item| item.to_string()).collect()),
        }
    }
    #[test]
    fn parse_valid_batch() {
        let cfg = parse_batch_file(write_temp(concat!(
            "[[task]]\nagent = \"gemini\"\nprompt = \"research X\"\nworktree = \"feat/x\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"implement Y\"\ndir = \"src\"\nmodel = \"gpt-4\"\ngroup = \"wg-demo\""
        )).path()).unwrap();
        assert_eq!(cfg.tasks.len(), 2);
        assert_eq!(cfg.tasks[0].agent, "gemini");
        assert_eq!(cfg.tasks[0].worktree, Some("feat/x".into()));
        assert_eq!(cfg.tasks[1].dir, Some("src".into()));
        assert_eq!(cfg.tasks[1].group.as_deref(), Some("wg-demo"));
    }
    #[test]
    fn parses_batch_with_dependencies() {
        let cfg = parse_batch_file(write_temp(concat!(
            "[[task]]\nname = \"foundation\"\nagent = \"codex\"\nprompt = \"shared types\"\n",
            "[[task]]\nname = \"feature-a\"\nagent = \"codex\"\nprompt = \"feature a\"\n",
            "depends_on = [\"foundation\"]\n"
        )).path()).unwrap();
        assert_eq!(cfg.tasks[0].name.as_deref(), Some("foundation"));
        assert_eq!(cfg.tasks[1].depends_on.as_deref(), Some(&["foundation".to_string()][..]));
    }
    #[test]
    fn rejects_unknown_agent() {
        let f = write_temp("[[task]]\nagent = \"gpt-3\"\nprompt = \"do something\"");
        assert!(parse_batch_file(f.path()).unwrap_err().to_string().contains("unknown agent"));
    }
    #[test]
    fn rejects_duplicate_worktree() {
        let f = write_temp(concat!(
            "[[task]]\nagent = \"gemini\"\nprompt = \"a\"\nworktree = \"feat/x\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/x\""
        ));
        assert!(parse_batch_file(f.path()).unwrap_err().to_string().contains("duplicate worktree"));
    }
    #[test]
    fn rejects_empty_batch() {
        let err = parse_batch_file(write_temp("").path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("parse TOML") || err.contains("no tasks"));
    }
    #[test]
    fn rejects_invalid_dependency_reference() {
        let err = validate_dag(&[make_task(Some("feature"), &["missing"])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown task"));
    }
    #[test]
    fn rejects_dependency_cycles() {
        let tasks = vec![
            make_task(Some("foundation"), &["integration"]),
            make_task(Some("integration"), &["foundation"]),
        ];
        let err = validate_dag(&tasks).unwrap_err().to_string();
        assert!(err.contains("cycle"));
    }
    #[test]
    fn topo_levels_group_parallel_work() {
        let tasks = vec![
            make_task(Some("foundation"), &[]),
            make_task(Some("feature-a"), &["foundation"]),
            make_task(Some("feature-b"), &["foundation"]),
            make_task(Some("integration"), &["feature-a", "feature-b"]),
        ];
        let levels = topo_levels(&tasks).unwrap();
        assert_eq!(levels, vec![vec![0], vec![1, 2], vec![3]]);
    }
}

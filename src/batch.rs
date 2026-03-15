// Batch task file parser: reads TOML batch configs and validates task DAGs.
// Each batch file declares tasks with agent, prompt, overrides, and dependencies.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const VALID_AGENTS: &[&str] = &["gemini", "codex", "opencode", "cursor", "kilo"];

fn deserialize_verify<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    use serde::de;
    struct VerifyVisitor;
    impl<'de> de::Visitor<'de> for VerifyVisitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or boolean")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(if v { Some("auto".to_string()) } else { None })
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }
    deserializer.deserialize_any(VerifyVisitor)
}

#[derive(Debug, Deserialize)]
pub struct BatchConfig {
    #[serde(default)]
    pub defaults: BatchDefaults,
    #[serde(alias = "task", alias = "tasks")]
    pub tasks: Vec<BatchTask>,
}

#[derive(Debug, Deserialize, Default)]
pub struct BatchDefaults {
    pub agent: Option<String>,
    pub team: Option<String>,
    pub dir: Option<String>,
    pub model: Option<String>,
    pub worktree_prefix: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    #[serde(default)]
    pub max_duration_mins: Option<u64>,
    pub context: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default)]
    pub read_only: Option<bool>,
    #[serde(default)]
    pub budget: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct BatchTask {
    pub name: Option<String>,
    #[serde(default)]
    pub agent: String,
    pub team: Option<String>,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub group: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    #[serde(default)]
    pub max_duration_mins: Option<u64>,
    pub context: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub context_from: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub budget: bool,
    pub on_success: Option<String>,
    pub on_fail: Option<String>,
    #[serde(default)]
    pub conditional: bool,
}

pub fn parse_batch_file(path: &Path) -> Result<BatchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read batch file: {}", path.display()))?;
    let mut config: BatchConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    if config.tasks.is_empty() {
        anyhow::bail!("batch file contains no tasks");
    }
    apply_defaults(&mut config.tasks, &config.defaults);
    validate_agents(&config.tasks)?;
    validate_fallback_agents(&config.tasks)?;
    validate_no_file_overlap(&config.tasks)?;
    validate_dag(&config.tasks)?;
    validate_conditional_hooks(&config.tasks)?;
    Ok(config)
}

fn apply_defaults(tasks: &mut [BatchTask], defaults: &BatchDefaults) {
    for task in tasks {
        apply_task_defaults(task, defaults);
    }
}
fn apply_task_defaults(task: &mut BatchTask, defaults: &BatchDefaults) {
    if task.agent.is_empty() {
        if let Some(agent) = defaults.agent.as_ref() {
            task.agent = agent.clone();
        }
    }
    if task.team.is_none() {
        task.team = defaults.team.clone();
    }
    if task.dir.is_none() {
        task.dir = defaults.dir.clone();
    }
    if task.model.is_none() {
        task.model = defaults.model.clone();
    }
    if task.worktree.is_none() {
        task.worktree = default_worktree(task, defaults);
    }
    if task.verify.is_none() {
        task.verify = defaults.verify.clone();
    }
    if task.max_duration_mins.is_none() {
        task.max_duration_mins = defaults.max_duration_mins;
    }
    if task.context.is_none() {
        task.context = defaults.context.clone();
    }
    if task.skills.is_none() {
        task.skills = defaults.skills.clone();
    }
    if task.hooks.is_none() {
        task.hooks = defaults.hooks.clone();
    }
    if task.fallback.is_none() {
        task.fallback = defaults.fallback.clone();
    }
    if !task.read_only && matches!(defaults.read_only, Some(true)) {
        task.read_only = true;
    }
    if !task.budget && matches!(defaults.budget, Some(true)) {
        task.budget = true;
    }
}
fn default_worktree(task: &BatchTask, defaults: &BatchDefaults) -> Option<String> {
    let prefix = defaults.worktree_prefix.as_deref()?;
    let name = task.name.as_deref()?.trim();
    (!name.is_empty()).then(|| format!("{prefix}/{name}"))
}
fn validate_agents(tasks: &[BatchTask]) -> Result<()> {
    for (task_idx, task) in tasks.iter().enumerate() {
        if task.agent.trim().is_empty() {
            // Allow empty agent when team is set (will auto-select from team)
            if task.team.is_some() {
                continue;
            }
            anyhow::bail!("task {} is missing agent", task_label(task, task_idx));
        }
        if task.agent != "auto" && !is_valid_agent(&task.agent) {
            anyhow::bail!("unknown agent: {}", task.agent);
        }
    }
    Ok(())
}
fn validate_fallback_agents(tasks: &[BatchTask]) -> Result<()> {
    for task in tasks {
        if let Some(fallback) = task.fallback.as_deref() {
            if !is_valid_agent(fallback) {
                anyhow::bail!("unknown fallback agent: {}", fallback);
            }
        }
    }
    Ok(())
}
pub(crate) fn is_valid_agent(agent: &str) -> bool {
    if VALID_AGENTS
        .iter()
        .any(|valid| valid.eq_ignore_ascii_case(agent))
    {
        return true;
    }
    crate::agent::registry::custom_agent_exists(agent)
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

fn validate_conditional_hooks(tasks: &[BatchTask]) -> Result<()> {
    let name_to_index = task_name_map(tasks)?;
    for (task_idx, task) in tasks.iter().enumerate() {
        if let Some(target) = task.on_success.as_deref() {
            validate_conditional_hook(target, "on_success", task_idx, tasks, &name_to_index)?;
        }
        if let Some(target) = task.on_fail.as_deref() {
            validate_conditional_hook(target, "on_fail", task_idx, tasks, &name_to_index)?;
        }
    }
    Ok(())
}

fn validate_conditional_hook(
    target: &str,
    hook_name: &str,
    task_idx: usize,
    tasks: &[BatchTask],
    name_to_index: &HashMap<&str, usize>,
) -> Result<()> {
    let trimmed = target.trim();
    anyhow::ensure!(!trimmed.is_empty(), "task {} has empty {} reference", task_label(&tasks[task_idx], task_idx), hook_name);
    let Some(&target_idx) = name_to_index.get(trimmed) else {
        anyhow::bail!("task {} references unknown task '{}' via {}", task_label(&tasks[task_idx], task_idx), trimmed, hook_name);
    };
    if !tasks[target_idx].conditional {
        anyhow::bail!(
            "task {} references {} via {} but target is not conditional",
            task_label(&tasks[task_idx], task_idx),
            task_label(&tasks[target_idx], target_idx),
            hook_name,
        );
    }
    Ok(())
}
pub(crate) fn dependency_indices(tasks: &[BatchTask]) -> Result<Vec<Vec<usize>>> {
    let name_to_index = task_name_map(tasks)?;
    tasks
        .iter()
        .enumerate()
        .map(|(task_idx, task)| resolve_dependencies(task_idx, task, &name_to_index))
        .collect()
}
pub(crate) fn task_name_map(tasks: &[BatchTask]) -> Result<HashMap<&str, usize>> {
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
enum VisitState {
    Pending,
    Visiting,
    Visited,
}
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
mod tests;

// Batch task file parser: reads TOML batch configs and validates task DAGs.
// Each batch file declares tasks with agent, prompt, overrides, and dependencies.
#[path = "batch_interpolate.rs"]
mod batch_interpolate;
#[path = "batch_serde.rs"]
mod batch_serde;
use self::batch_interpolate::{apply_defaults, interpolate_batch_config};
use self::batch_serde::{deserialize_judge, deserialize_string_or_vec, deserialize_verify};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfig {
    #[serde(default)]
    pub defaults: BatchDefaults,
    #[serde(default)]
    pub vars: HashMap<String, String>,
    #[serde(alias = "task", alias = "tasks")]
    pub tasks: Vec<BatchTask>,
}
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BatchDefaults {
    pub group_id: Option<String>,
    pub group: Option<String>,
    #[serde(default)]
    pub shared_dir: Option<bool>,
    #[serde(default)]
    pub analyze: Option<bool>,
    pub agent: Option<String>,
    #[serde(default)]
    pub auto_fallback: Option<bool>,
    pub team: Option<String>,
    pub dir: Option<String>,
    pub model: Option<String>,
    pub worktree_prefix: Option<String>,
    #[serde(default, deserialize_with = "deserialize_judge")]
    pub judge: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub max_duration_mins: Option<u64>,
    #[serde(default)]
    pub idle_timeout: Option<u64>,
    #[serde(default)]
    pub best_of: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub scope: Option<Vec<String>>,
    #[serde(default)]
    pub read_only: Option<bool>,
    #[serde(default)]
    pub budget: Option<bool>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub env_forward: Option<Vec<String>>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchTask {
    pub id: Option<String>,
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
    pub container: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    #[serde(default, deserialize_with = "deserialize_judge")]
    pub judge: Option<String>,
    #[serde(default)]
    pub best_of: Option<usize>,
    #[serde(default)]
    pub max_duration_mins: Option<u64>,
    #[serde(default)]
    pub idle_timeout: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub parent: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context_from: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub scope: Option<Vec<String>>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub budget: bool,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub env_forward: Option<Vec<String>>,
    pub on_success: Option<String>,
    pub on_fail: Option<String>,
    #[serde(default)]
    pub conditional: bool,
}

pub fn parse_batch_file(path: &Path) -> Result<BatchConfig> {
    parse_batch_file_with_vars(path, &HashMap::new())
}

fn validate_batch_keys(content: &str, path: &Path) -> Result<()> {
    let raw: toml::Value = toml::from_str(content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    let Some(table) = raw.as_table() else {
        return Ok(());
    };
    let known_keys = ["defaults", "tasks", "task", "vars"];
    for key in table.keys() {
        if known_keys.contains(&key.as_str()) {
            continue;
        }
        let suggestion = match key.as_str() {
            "default" => " (did you mean `[defaults]`?)",
            _ => "",
        };
        anyhow::bail!(
            "unknown top-level key `{}` in batch file {}{}",
            key,
            path.display(),
            suggestion
        );
    }
    Ok(())
}

pub(crate) fn parse_batch_file_with_vars(
    path: &Path,
    cli_vars: &HashMap<String, String>,
) -> Result<BatchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read batch file: {}", path.display()))?;
    validate_batch_keys(&content, path)?;
    let mut config: BatchConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    if config.tasks.is_empty() {
        anyhow::bail!("batch file contains no tasks");
    }
    let mut stderr = io::stderr().lock();
    interpolate_batch_config(&mut config, cli_vars, &mut stderr)?;
    apply_defaults(&mut config.tasks, &config.defaults);
    validate_agents(&config.tasks)?;
    validate_fallback_agents(&config.tasks)?;
    auto_sequence_shared_worktrees(&mut config.tasks, &mut stderr)?;
    warn_prompt_size(&config.tasks, &mut stderr)?;
    validate_dag(&config.tasks)?;
    validate_conditional_hooks(&config.tasks)?;
    warn_audit_without_readonly(&config.tasks);
    for warning in warn_dir_overlap(&config.tasks) {
        eprintln!("{}", warning);
    }
    Ok(config)
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
        if let Some(judge_agent) = task.judge.as_deref() && !judge_agent.trim().is_empty() && !is_valid_agent(judge_agent) {
            anyhow::bail!("unknown judge agent: {}", judge_agent);
        }
    }
    Ok(())
}
fn validate_fallback_agents(tasks: &[BatchTask]) -> Result<()> {
    for task in tasks {
        if let Some(fallback) = task.fallback.as_deref() {
            for agent in fallback.split(',').map(str::trim) {
                if !agent.is_empty() && !is_valid_agent(agent) {
                    anyhow::bail!("unknown fallback agent: {}", agent);
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn is_valid_agent(agent: &str) -> bool {
    crate::types::AgentKind::parse_str(agent).is_some() || crate::agent::registry::custom_agent_exists(agent)
}
pub fn auto_sequence_shared_worktrees(tasks: &mut [BatchTask], writer: &mut impl Write) -> io::Result<()> {
    // Validate: all tasks sharing a worktree must have names for dependency tracking.
    // Without names, depends_on cannot reference them, breaking the sequencing chain
    // and allowing concurrent git access that causes index.lock contention.
    let mut worktree_users: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, task) in tasks.iter().enumerate() {
        if let Some(wt) = task.worktree.as_deref() {
            worktree_users.entry(wt).or_default().push(idx);
        }
    }
    for (wt, indices) in &worktree_users {
        if indices.len() < 2 { continue; }
        for &idx in indices {
            if tasks[idx].name.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "task #{idx} shares worktree '{wt}' with {} other task(s) but has no name — \
                         add name = \"...\" so aid can auto-sequence shared worktree access",
                        indices.len() - 1
                    ),
                ));
            }
        }
    }

    // Add sequential dependencies for shared worktrees
    let mut last_task_by_worktree: HashMap<String, String> = HashMap::new();
    for (task_idx, task) in tasks.iter_mut().enumerate() {
        let Some(worktree) = task.worktree.as_ref() else {
            continue;
        };
        let current_label = task_label(task, task_idx);
        if let Some(previous_task) = last_task_by_worktree.get(worktree).cloned() {
            let depends_on = task.depends_on.get_or_insert_with(Vec::new);
            if !depends_on.iter().any(|dependency| dependency == &previous_task) {
                depends_on.push(previous_task.clone());
            }
            writeln!(writer, "[aid] Warning: task '{}' shares worktree '{}' with '{}'; auto-sequencing execution.", current_label, worktree, previous_task)?;
        }
        if let Some(name) = task.name.as_ref() {
            last_task_by_worktree.insert(worktree.clone(), name.clone());
        }
    }
    Ok(())
}
fn warn_prompt_size(tasks: &[BatchTask], writer: &mut impl Write) -> io::Result<()> {
    for (idx, task) in tasks.iter().enumerate() {
        let chars = task.prompt.len();
        if chars > 6000 {
            let lines = task.prompt.lines().count();
            writeln!(
                writer,
                "[aid] Warning: task '{}' has a large prompt (~{} chars, {} lines). Consider splitting into smaller tasks for better agent execution quality.",
                task_label(task, idx),
                chars,
                lines,
            )?;
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
        anyhow::bail!("task {} references {} via {} but target is not conditional", task_label(&tasks[task_idx], task_idx), task_label(&tasks[target_idx], target_idx), hook_name);
    }
    Ok(())
}
pub(crate) fn dependency_indices(tasks: &[BatchTask]) -> Result<Vec<Vec<usize>>> {
    let name_to_index = task_name_map(tasks)?;
    tasks.iter().enumerate().map(|(task_idx, task)| resolve_dependencies(task_idx, task, &name_to_index)).collect()
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
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    // Explicit depends_on
    if let Some(depends_on) = task.depends_on.as_ref() {
        for dependency_name in depends_on {
            let trimmed = dependency_name.trim();
            anyhow::ensure!(
                !trimmed.is_empty(),
                "task {} has an empty dependency reference",
                task_label(task, task_idx)
            );
            let Some(&dependency_idx) = name_to_index.get(trimmed) else {
                anyhow::bail!("task {} depends on unknown task: {}", task_label(task, task_idx), trimmed);
            };
            if seen.insert(dependency_idx) {
                resolved.push(dependency_idx);
            }
        }
    }
    // context_from implies dependency — task must complete before its output is readable
    if let Some(context_from) = task.context_from.as_ref() {
        for source_name in context_from {
            let trimmed = source_name.trim();
            if let Some(&source_idx) = name_to_index.get(trimmed) && seen.insert(source_idx) {
                resolved.push(source_idx);
            }
        }
    }
    Ok(resolved)
}
fn task_label(task: &BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}
pub fn warn_dir_overlap(tasks: &[BatchTask]) -> Vec<String> {
    let mut dir_counts: HashMap<&str, usize> = HashMap::new();
    for task in tasks {
        if task.worktree.is_some() {
            continue;
        }
        if let Some(ref dir) = task.dir {
            *dir_counts.entry(dir.as_str()).or_default() += 1;
        }
    }
    let mut warnings = Vec::new();
    for (dir, count) in &dir_counts {
        if *count > 1 {
            warnings.push(format!(
                "[aid] ⚠ {} tasks target dir '{}' without worktree isolation — risk of git index.lock contention",
                count, dir
            ));
            warnings.push("[aid] Tip: add `worktree = \"branch-name\"` to each task for safe parallel execution".to_string());
        }
    }
    warnings
}
pub fn warn_audit_without_readonly(tasks: &[BatchTask]) {
    let _ = warn_audit_without_readonly_into(tasks, &mut io::stderr().lock());
}
fn warn_audit_without_readonly_into(tasks: &[BatchTask], writer: &mut impl Write) -> io::Result<()> {
    for (task_idx, task) in tasks.iter().enumerate() {
        if task.read_only || !prompt_suggests_read_only(&task.prompt) {
            continue;
        }
        writeln!(
            writer,
            "[aid] ⚠ Task '{}' prompt suggests read-only intent but read_only is not set. Consider adding read_only = true",
            task_label(task, task_idx)
        )?;
    }
    Ok(())
}
fn prompt_suggests_read_only(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    lower.contains("do not modify")
        || lower.contains("don't modify")
        || lower.contains("report only")
        || lower.contains("read only")
        || lower.contains("read-only")
        || lower.contains("do not change")
        || lower.contains("analysis only")
        || lower.contains("analyze only")
        || (lower.contains("audit") && !lower.contains("audit trail") && !lower.contains("audit log"))
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
        VisitState::Visiting => anyhow::bail!("dependency cycle detected at task {}", task_label(&tasks[task_idx], task_idx)),
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

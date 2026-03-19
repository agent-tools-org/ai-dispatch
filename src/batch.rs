// Batch task file parser: reads TOML batch configs and validates task DAGs.
// Each batch file declares tasks with agent, prompt, overrides, and dependencies.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;


fn deserialize_judge<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    use serde::de;
    struct JudgeVisitor;
    impl<'de> de::Visitor<'de> for JudgeVisitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or string")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(if v { Some("gemini".to_string()) } else { None })
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
    deserializer.deserialize_any(JudgeVisitor)
}

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
    #[serde(default)]
    pub vars: HashMap<String, String>,
    #[serde(alias = "task", alias = "tasks")]
    pub tasks: Vec<BatchTask>,
}

#[derive(Debug, Deserialize, Default)]
pub struct BatchDefaults {
    pub group_id: Option<String>,
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
    pub best_of: Option<usize>,
    pub context: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub fallback: Option<String>,
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
    pub context: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub parent: Option<String>,
    pub context_from: Option<Vec<String>>,
    pub fallback: Option<String>,
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

pub(crate) fn parse_batch_file_with_vars(
    path: &Path,
    cli_vars: &HashMap<String, String>,
) -> Result<BatchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read batch file: {}", path.display()))?;
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
    validate_no_file_overlap(&config.tasks)?;
    validate_dag(&config.tasks)?;
    validate_conditional_hooks(&config.tasks)?;
    warn_audit_without_readonly(&config.tasks);
    for warning in warn_dir_overlap(&config.tasks) {
        eprintln!("{}", warning);
    }
    Ok(config)
}

fn interpolate_batch_config(
    config: &mut BatchConfig,
    cli_vars: &HashMap<String, String>,
    writer: &mut impl Write,
) -> io::Result<()> {
    let mut vars = config.vars.clone();
    vars.extend(cli_vars.clone());
    for task in &mut config.tasks {
        interpolate_task(task, &vars, writer)?;
    }
    Ok(())
}

fn interpolate_task(
    task: &mut BatchTask,
    vars: &HashMap<String, String>,
    writer: &mut impl Write,
) -> io::Result<()> {
    interpolate_string(&mut task.id, vars, writer)?;
    interpolate_string(&mut task.name, vars, writer)?;
    interpolate_plain_string(&mut task.agent, vars, writer)?;
    interpolate_string(&mut task.team, vars, writer)?;
    interpolate_plain_string(&mut task.prompt, vars, writer)?;
    interpolate_string(&mut task.dir, vars, writer)?;
    interpolate_string(&mut task.output, vars, writer)?;
    interpolate_string(&mut task.model, vars, writer)?;
    interpolate_string(&mut task.worktree, vars, writer)?;
    interpolate_string(&mut task.group, vars, writer)?;
    interpolate_string(&mut task.verify, vars, writer)?;
    interpolate_string(&mut task.judge, vars, writer)?;
    interpolate_vec(&mut task.context, vars, writer)?;
    interpolate_vec(&mut task.skills, vars, writer)?;
    interpolate_vec(&mut task.hooks, vars, writer)?;
    interpolate_vec(&mut task.depends_on, vars, writer)?;
    interpolate_string(&mut task.parent, vars, writer)?;
    interpolate_vec(&mut task.context_from, vars, writer)?;
    interpolate_string(&mut task.fallback, vars, writer)?;
    interpolate_vec(&mut task.scope, vars, writer)?;
    interpolate_string(&mut task.on_success, vars, writer)?;
    interpolate_string(&mut task.on_fail, vars, writer)?;
    Ok(())
}

fn interpolate_vec(
    values: &mut Option<Vec<String>>,
    vars: &HashMap<String, String>,
    writer: &mut impl Write,
) -> io::Result<()> {
    if let Some(values) = values {
        for value in values {
            interpolate_plain_string(value, vars, writer)?;
        }
    }
    Ok(())
}

fn interpolate_string(
    value: &mut Option<String>,
    vars: &HashMap<String, String>,
    writer: &mut impl Write,
) -> io::Result<()> {
    if let Some(value) = value {
        interpolate_plain_string(value, vars, writer)?;
    }
    Ok(())
}

fn interpolate_plain_string(
    value: &mut String,
    vars: &HashMap<String, String>,
    writer: &mut impl Write,
) -> io::Result<()> {
    let mut cursor = 0;
    let mut output = String::with_capacity(value.len());
    while let Some(start_rel) = value[cursor..].find("{{") {
        let start = cursor + start_rel;
        output.push_str(&value[cursor..start]);
        let search_from = start + 2;
        if let Some(end_rel) = value[search_from..].find("}}") {
            let end = search_from + end_rel;
            let key = value[search_from..end].trim();
            if let Some(replacement) = vars.get(key) {
                output.push_str(replacement);
            } else {
                writeln!(writer, "[aid] Warning: missing batch var '{key}'")?;
                output.push_str(&value[start..end + 2]);
            }
            cursor = end + 2;
        } else {
            output.push_str(&value[start..]);
            cursor = value.len();
        }
    }
    output.push_str(&value[cursor..]);
    *value = output;
    Ok(())
}

fn apply_defaults(tasks: &mut [BatchTask], defaults: &BatchDefaults) {
    for task in tasks {
        apply_task_defaults(task, defaults);
    }
}
fn apply_task_defaults(task: &mut BatchTask, defaults: &BatchDefaults) {
    if task.agent.is_empty()
        && let Some(agent) = defaults.agent.as_ref()
    {
        task.agent = agent.clone();
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
    if task.container.is_none() {
        task.container = defaults.container.clone();
    }
    if task.judge.is_none() {
        task.judge = defaults.judge.clone();
    }
    if task.max_duration_mins.is_none() {
        task.max_duration_mins = defaults.max_duration_mins;
    }
    if task.best_of.is_none() {
        task.best_of = defaults.best_of;
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
    if task.scope.is_none() {
        task.scope = defaults.scope.clone();
    }
    if !task.read_only && matches!(defaults.read_only, Some(true)) {
        task.read_only = true;
    }
    if !task.budget && matches!(defaults.budget, Some(true)) {
        task.budget = true;
    }
    task.env = merge_env_maps(defaults.env.as_ref(), task.env.as_ref());
    task.env_forward = merge_env_lists(defaults.env_forward.as_ref(), task.env_forward.as_ref());
}
fn default_worktree(task: &BatchTask, defaults: &BatchDefaults) -> Option<String> {
    let prefix = defaults.worktree_prefix.as_deref()?;
    let name = task.name.as_deref()?.trim();
    (!name.is_empty()).then(|| format!("{prefix}/{name}"))
}

fn merge_env_maps(
    defaults: Option<&HashMap<String, String>>,
    task: Option<&HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    let mut merged = defaults.cloned().unwrap_or_default();
    if let Some(task) = task {
        merged.extend(task.clone());
    }
    (!merged.is_empty()).then_some(merged)
}

fn merge_env_lists(defaults: Option<&Vec<String>>, task: Option<&Vec<String>>) -> Option<Vec<String>> {
    let mut merged = defaults.cloned().unwrap_or_default();
    if let Some(task) = task {
        merged.extend(task.iter().cloned());
    }
    (!merged.is_empty()).then_some(merged)
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
        if let Some(judge_agent) = task.judge.as_deref()
            && !judge_agent.trim().is_empty()
            && !is_valid_agent(judge_agent)
        {
            anyhow::bail!("unknown judge agent: {}", judge_agent);
        }
    }
    Ok(())
}
fn validate_fallback_agents(tasks: &[BatchTask]) -> Result<()> {
    for task in tasks {
        if let Some(fallback) = task.fallback.as_deref()
            && !is_valid_agent(fallback)
        {
            anyhow::bail!("unknown fallback agent: {}", fallback);
        }
    }
    Ok(())
}
pub(crate) fn is_valid_agent(agent: &str) -> bool {
    crate::types::AgentKind::parse_str(agent).is_some()
        || crate::agent::registry::custom_agent_exists(agent)
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
    }
    // context_from implies dependency — task must complete before its output is readable
    if let Some(context_from) = task.context_from.as_ref() {
        for source_name in context_from {
            let trimmed = source_name.trim();
            if let Some(&source_idx) = name_to_index.get(trimmed)
                && seen.insert(source_idx) {
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
        || (lower.contains("audit")
            && !lower.contains("audit trail")
            && !lower.contains("audit log"))
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

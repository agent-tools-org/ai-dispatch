// Batch variable interpolation and default propagation helpers.
// Exports: parent-visible config interpolation and task default application.
// Deps: batch config types, std collections, std::io::Write.

use crate::batch::{BatchConfig, BatchDefaults, BatchTask};
use std::collections::HashMap;
use std::io::{self, Write};

pub(super) fn interpolate_batch_config(
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
    interpolate_string(&mut task.setup, vars, writer)?;
    interpolate_string(&mut task.judge, vars, writer)?;
    interpolate_string(&mut task.peer_review, vars, writer)?;
    interpolate_string(&mut task.eval, vars, writer)?;
    interpolate_string(&mut task.eval_feedback_template, vars, writer)?;
    interpolate_string(&mut task.metric, vars, writer)?;
    interpolate_vec(&mut task.context, vars, writer)?;
    interpolate_vec(&mut task.checklist, vars, writer)?;
    interpolate_vec(&mut task.skills, vars, writer)?;
    interpolate_string(&mut task.on_done, vars, writer)?;
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

pub(super) fn apply_defaults(tasks: &mut [BatchTask], defaults: &BatchDefaults) {
    for (idx, task) in tasks.iter_mut().enumerate() {
        apply_task_defaults(task, defaults, idx);
    }
}

fn apply_task_defaults(task: &mut BatchTask, defaults: &BatchDefaults, task_idx: usize) {
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
        task.worktree = default_worktree(task, defaults, task_idx);
    }
    if task.verify.is_none() {
        task.verify = defaults.verify.clone();
    }
    if task.setup.is_none() {
        task.setup = defaults.setup.clone();
    }
    if task.container.is_none() {
        task.container = defaults.container.clone();
    }
    if task.judge.is_none() {
        task.judge = defaults.judge.clone();
    }
    if task.peer_review.is_none() {
        task.peer_review = defaults.peer_review.clone();
    }
    if task.max_duration_mins.is_none() {
        task.max_duration_mins = defaults.max_duration_mins;
    }
    if task.max_wait_mins.is_none() {
        task.max_wait_mins = defaults.max_wait_mins;
    }
    if task.retry.is_none() {
        task.retry = defaults.retry;
    }
    if task.iterate.is_none() {
        task.iterate = defaults.iterate;
    }
    if task.eval.is_none() {
        task.eval = defaults.eval.clone();
    }
    if task.eval_feedback_template.is_none() {
        task.eval_feedback_template = defaults.eval_feedback_template.clone();
    }
    if task.idle_timeout.is_none() {
        task.idle_timeout = defaults.idle_timeout;
    }
    if task.best_of.is_none() {
        task.best_of = defaults.best_of;
    }
    if task.metric.is_none() {
        task.metric = defaults.metric.clone();
    }
    if task.context.is_none() {
        task.context = defaults.context.clone();
    }
    if task.skills.is_none() {
        task.skills = defaults.skills.clone();
    }
    if task.on_done.is_none() {
        task.on_done = defaults.on_done.clone();
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
    if !task.sandbox && matches!(defaults.sandbox, Some(true)) {
        task.sandbox = true;
    }
    if !task.no_skill && matches!(defaults.no_skill, Some(true)) {
        task.no_skill = true;
    }
    if !task.budget && matches!(defaults.budget, Some(true)) {
        task.budget = true;
    }
    task.env = merge_env_maps(defaults.env.as_ref(), task.env.as_ref());
    task.env_forward = merge_env_lists(defaults.env_forward.as_ref(), task.env_forward.as_ref());
    if task.worktree_link_deps.is_none() {
        task.worktree_link_deps = defaults.worktree_link_deps;
    }
}

fn default_worktree(task: &BatchTask, defaults: &BatchDefaults, task_idx: usize) -> Option<String> {
    let prefix = defaults.worktree_prefix.as_deref()?;
    match task.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => Some(format!("{prefix}/{name}")),
        None => Some(format!("{prefix}/task-{task_idx}")),
    }
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

fn merge_env_lists(
    defaults: Option<&Vec<String>>,
    task: Option<&Vec<String>>,
) -> Option<Vec<String>> {
    let mut merged = defaults.cloned().unwrap_or_default();
    if let Some(task) = task {
        merged.extend(task.iter().cloned());
    }
    (!merged.is_empty()).then_some(merged)
}

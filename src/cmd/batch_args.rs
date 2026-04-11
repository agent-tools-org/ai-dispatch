// Batch task -> run args conversion helpers.
// Exports: task_to_run_args
// Deps: crate::cmd::run::RunArgs, crate::batch, crate::store::Store
use crate::batch;
use crate::cmd::run::{RunArgs, NO_SKILL_SENTINEL};
use crate::store::Store;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn task_to_run_args(
    task: &batch::BatchTask,
    siblings: &[&batch::BatchTask],
    background: bool,
    store: &Arc<Store>,
    shared_dir_path: Option<&str>,
) -> RunArgs {
    // If team is set and agent is empty/auto, auto-select from team members
    let agent_name = if (task.agent.is_empty() || task.agent == "auto") && task.team.is_some() {
        let team_config = task.team.as_deref().and_then(crate::team::resolve_team);
        let selection_opts = crate::agent::RunOpts {
            dir: task.dir.clone(),
            output: task.output.clone(),
            result_file: auto_scope_result_file(task, siblings),
            model: task.model.clone(),
            budget: task.budget,
            read_only: task.read_only,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let (selected, reason) = crate::agent::select_agent_with_reason(
            &task.prompt,
            &selection_opts,
            store,
            team_config.as_ref(),
        );
        aid_info!("[aid] Batch auto-selected: {selected} (reason: {reason})");
        selected
    } else if task.agent.is_empty() {
        "auto".to_string()
    } else {
        task.agent.clone()
    };
    let batch_siblings = siblings
        .iter()
        .map(|sibling| {
            (
                sibling
                    .name
                    .clone()
                    .or_else(|| sibling.id.clone())
                    .unwrap_or_else(|| "<unnamed>".to_string()),
                if sibling.agent.is_empty() {
                    "auto".to_string()
                } else {
                    sibling.agent.clone()
                },
                sibling.prompt.clone(),
            )
        })
        .collect();
    let cascade = task
        .fallback
        .as_deref()
        .map(|fallback| {
            fallback
                .split(',')
                .map(str::trim)
                .filter(|agent| !agent.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_else(|| auto_cascade_for_rate_limited(&agent_name));
    let env = crate::idle_timeout::env_with_idle_timeout(
        merged_env(task.env.as_ref(), task.env_forward.as_ref(), shared_dir_path),
        task.idle_timeout,
    );
    let skills = if task.no_skill {
        vec![NO_SKILL_SENTINEL.to_string()]
    } else {
        task.skills.clone().unwrap_or_default()
    };
    RunArgs {
        agent_name,
        prompt: task.prompt.clone(),
        dir: task.dir.clone(),
        output: task.output.clone(),
        result_file: auto_scope_result_file(task, siblings),
        model: task.model.clone(),
        worktree: task.worktree.clone(),
        group: task.group.clone(),
        container: task.container.clone(),
        verify: task.verify.clone(),
        setup: task.setup.clone(),
        iterate: task.iterate,
        eval: task.eval.clone(),
        eval_feedback_template: task.eval_feedback_template.clone(),
        judge: task.judge.clone(),
        peer_review: task.peer_review.clone(),
        max_duration_mins: task.max_duration_mins.map(|value| value as i64),
        retry: task.retry.unwrap_or(0),
        context: task.context.clone().unwrap_or_default(),
        checklist: task.checklist.clone().unwrap_or_default(),
        skills,
        hooks: task.hooks.clone().unwrap_or_default(),
        background,
        dry_run: false,
        announce: true,
        on_done: task.on_done.clone(),
        cascade,
        read_only: task.read_only,
        sandbox: task.sandbox,
        budget: task.budget,
        best_of: task.best_of,
        metric: task.metric.clone(),
        team: task.team.clone(),
        context_from: task.context_from.clone().unwrap_or_default(),
        batch_siblings,
        scope: task.scope.clone().unwrap_or_default(),
        parent_task_id: task.parent.clone(),
        existing_task_id: task.id.as_ref().map(|id| crate::types::TaskId(id.clone())),
        env,
        env_forward: task.env_forward.clone(),
        link_deps: task.worktree_link_deps.unwrap_or(true),
        ..Default::default()
    }
}

/// If the agent is rate-limited, return the suggested fallback as an auto-cascade.
fn auto_cascade_for_rate_limited(agent_name: &str) -> Vec<String> {
    let Some(agent) = crate::types::AgentKind::parse_str(agent_name) else {
        return vec![];
    };
    if !crate::rate_limit::is_rate_limited(&agent) {
        return vec![];
    }
    crate::agent::selection::coding_fallback_for(&agent)
        .map(|fallback| vec![fallback.as_str().to_string()])
        .unwrap_or_default()
}

/// Auto-scope result_file when sibling tasks share the same filename.
/// Appends `-{task_name}` before the extension to prevent parallel overwrites.
fn auto_scope_result_file(task: &batch::BatchTask, siblings: &[&batch::BatchTask]) -> Option<String> {
    let result_file = task.result_file.as_deref()?;
    let has_collision = siblings.iter().any(|s| s.result_file.as_deref() == Some(result_file));
    if !has_collision {
        return Some(result_file.to_string());
    }
    let task_name = task.name.as_deref()
        .or(task.id.as_deref())
        .unwrap_or("task");
    let scoped = scope_filename(result_file, task_name);
    aid_info!("[aid] Auto-scoped result_file: {result_file} → {scoped} (collision with sibling)");
    Some(scoped)
}

fn scope_filename(path: &str, suffix: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}-{suffix}.{ext}"),
        None => format!("{path}-{suffix}"),
    }
}

fn merged_env(
    env: Option<&HashMap<String, String>>,
    env_forward: Option<&Vec<String>>,
    shared_dir_path: Option<&str>,
) -> Option<HashMap<String, String>> {
    let mut merged = env.cloned().unwrap_or_default();
    if let Some(shared_dir_path) = shared_dir_path {
        merged.insert("AID_SHARED_DIR".to_string(), shared_dir_path.to_string());
    }
    if let Some(env_forward) = env_forward {
        for name in env_forward {
            if let Ok(value) = std::env::var(name) {
                merged.insert(name.clone(), value);
            }
        }
    }
    (!merged.is_empty()).then_some(merged)
}

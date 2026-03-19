// Batch task -> run args conversion helpers.
// Exports: task_to_run_args
// Deps: crate::cmd::run::RunArgs, crate::batch, crate::store::Store
use crate::batch;
use crate::cmd::run::RunArgs;
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
        .map(|f| vec![f.to_string()])
        .unwrap_or_else(|| auto_cascade_for_rate_limited(&agent_name));
    let env = merged_env(task.env.as_ref(), task.env_forward.as_ref(), shared_dir_path);
    RunArgs {
        agent_name,
        prompt: task.prompt.clone(),
        dir: task.dir.clone(),
        output: task.output.clone(),
        model: task.model.clone(),
        worktree: task.worktree.clone(),
        group: task.group.clone(),
        verify: task.verify.clone(),
        judge: task.judge.clone(),
        max_duration_mins: task.max_duration_mins.map(|value| value as i64),
        context: task.context.clone().unwrap_or_default(),
        skills: task.skills.clone().unwrap_or_default(),
        hooks: task.hooks.clone().unwrap_or_default(),
        background,
        dry_run: false,
        announce: true,
        cascade,
        read_only: task.read_only,
        budget: task.budget,
        best_of: task.best_of,
        team: task.team.clone(),
        context_from: task.context_from.clone().unwrap_or_default(),
        batch_siblings,
        scope: task.scope.clone().unwrap_or_default(),
        parent_task_id: task.parent.clone(),
        env,
        env_forward: task.env_forward.clone(),
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

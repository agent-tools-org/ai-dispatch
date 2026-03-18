// Batch task -> run args conversion helpers.
// Exports: task_to_run_args
// Deps: crate::cmd::run::RunArgs, crate::batch, crate::store::Store
use crate::batch;
use crate::cmd::run::RunArgs;
use crate::store::Store;
use std::sync::Arc;

pub(crate) fn task_to_run_args(
    task: &batch::BatchTask,
    siblings: &[&batch::BatchTask],
    background: bool,
    store: &Arc<Store>,
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
        };
        let (selected, reason) =
            crate::agent::select_agent_with_reason(&task.prompt, &selection_opts, store, team_config.as_ref());
        eprintln!("[aid] Batch auto-selected: {selected} (reason: {reason})");
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
        cascade: task
            .fallback
            .as_deref()
            .map(|f| vec![f.to_string()])
            .unwrap_or_default(),
        read_only: task.read_only,
        budget: task.budget,
        best_of: task.best_of,
        team: task.team.clone(),
        context_from: task.context_from.clone().unwrap_or_default(),
        batch_siblings,
        scope: task.scope.clone().unwrap_or_default(),
        parent_task_id: task.parent.clone(),
        ..Default::default()
    }
}

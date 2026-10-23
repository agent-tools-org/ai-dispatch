// Cascade validation and per-task dispatch preparation.
// Deps: parent module types and helpers.
use super::*;

pub(super) fn resolve_cascade_agents(names: &[&str]) -> Result<Vec<(AgentKind, String)>> {
    let all: Vec<(AgentKind, String)> = names
        .iter()
        .map(|s| {
            AgentKind::parse_str(s)
                .map(|k| (k, (*s).to_string()))
                .or_else(|| {
                    crate::agent::registry::custom_agent_exists(s)
                        .then(|| (AgentKind::Custom, (*s).to_string()))
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unknown cascade agent '{s}'. Use `aid config agents` to list available agents."
                    )
                })
        })
        .collect::<Result<_>>()?;
    Ok(all)
}

pub(super) fn prepare_dispatch_task(
    store: &Arc<Store>,
    tasks: &[batch::BatchTask],
    task_idx: usize,
    waiting_ids: &[String],
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<(String, run::RunArgs)> {
    let siblings: Vec<_> = tasks
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx != task_idx)
        .map(|(_, task)| task)
        .collect();
    let mut run_args = task_to_run_args(&tasks[task_idx], &siblings, true, store, shared_dir_path);
    run_args.repo_root = repo_root.map(str::to_string);
    run_args.suppress_nested_repo_warning = true;
    run_args.existing_task_id = Some(crate::types::TaskId(waiting_ids[task_idx].clone()));
    if let Some((fallback_agent, remaining_cascade)) =
        pre_dispatch_fallback_choice(&run_args.agent_name, tasks[task_idx].fallback.as_deref())?
    {
        aid_info!(
            "[batch] {} rate-limited → using fallback: {} for task '{}'",
            run_args.agent_name,
            fallback_agent,
            dispatch_task_ref(&tasks[task_idx], task_idx),
        );
        crate::cmd::run::switch_agent(&mut run_args, fallback_agent);
        run_args.cascade = remaining_cascade;
    }
    let progress_ref = format!(
        "{}: {}",
        run_args.agent_name,
        dispatch_task_ref(&tasks[task_idx], task_idx),
    );
    Ok((progress_ref, run_args))
}

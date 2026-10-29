// Dispatch claim completion and preparation failure settlement.
// Uses the conditional status transaction to retain the original error atomically.
use super::*;

pub(crate) fn record_error(store: &Store, task_id: &TaskId, err: &anyhow::Error) -> Result<()> {
    store.settle_worker(task_id.as_str(), "launch_failed", Some(&err.to_string()))?;
    let event = TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: format!("{err:#}"),
        metadata: Some(serde_json::json!({"failure_kind": "preparation"})),
    };
    if store.settle_task_status(
        task_id.as_str(),
        TaskStatus::Failed,
        Some(&[TaskStatus::Pending, TaskStatus::Running]),
        Some(PendingReason::Preparation),
        Some(&event),
    )? {
        crate::task::lifecycle::salvage_failed_task(store, task_id);
    }
    Ok(())
}

pub(super) fn finish_claim(
    store: &Arc<Store>,
    args: &RunArgs,
    context: &DispatchContext,
    task_id: &TaskId,
) -> Result<()> {
    let result = (|| -> Result<()> {
        if args.origin == DispatchOrigin::ExplicitRetry {
            store.update_task_dispatch_args(task_id.as_str(), &args.dispatch_args_json()?)?;
        }
        maybe_insert_held_route_event(store, task_id, &context.agent_setup, args.dry_run);
        let setup = &context.agent_setup;
        let priced_model = setup
            .effective_model
            .clone()
            .or_else(|| setup.agent.default_model());
        super::super::run_dispatch_cost_ceiling::warn_unenforceable_cost_ceiling(
            store,
            task_id,
            args.max_task_cost,
            setup.agent_kind,
            &setup.agent_display_name,
            priced_model.as_deref(),
        );
        persist_declaration(store, task_id, args)?;
        Ok(())
    })();
    if let Err(err) = result {
        record_error(store, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}

pub(super) fn prepared_dispatch(
    requested_skills: Vec<String>,
    project_root: Option<PathBuf>,
    detected_project: Option<ProjectConfig>,
    agent_setup: AgentSetup,
    task_id: TaskId,
    task: Task,
    workgroup: Option<Workgroup>,
    setup: WorktreeSetup,
) -> PreparedDispatch {
    PreparedDispatch {
        detected_project,
        project_root,
        agent_kind: agent_setup.agent_kind,
        agent_display_name: agent_setup.agent_display_name,
        requested_skills,
        effective_model: agent_setup.effective_model,
        budget_active: agent_setup.budget_active,
        task_id,
        task,
        workgroup,
        wt_path: setup.wt_path,
        effective_dir: setup.effective_dir,
    }
}

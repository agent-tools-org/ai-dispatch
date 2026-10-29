// Post-run phase ordering and retry/cascade continuation.
// Exports run_lifecycle_phases; deps: parent lifecycle helpers and task state.
use super::*;

use std::ops::ControlFlow;

#[path = "phases_finish.rs"]
mod phases_finish;

pub(super) async fn run_lifecycle_phases(
    mode: LifecycleMode,
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    agent_kind: AgentKind,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    repo_path: Option<&String>,
    wt_path: Option<&String>,
    container_name: Option<&str>,
    runtime_hooks: &[hooks::Hook],
    prompt_bundle: &run_prompt::PromptBundle,
    pre_verify_status: TaskStatus,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<Option<TaskId>> {
    let mut run = LifecycleRun::new(
        mode,
        store,
        task_id,
        args,
        agent_display_name,
        effective_dir,
        repo_path,
        runtime_hooks,
    );
    if let ControlFlow::Break(outcome) = run
        .prepare(
            wt_path,
            pre_task_dirty_paths,
            container_name,
            agent_kind,
            prompt_bundle,
        )
        .await?
    {
        return Ok(outcome);
    }
    if let ControlFlow::Break(outcome) = run.complete().await? {
        return Ok(outcome);
    }
    if let ControlFlow::Break(outcome) = run.retry_verification(pre_verify_status).await? {
        return Ok(outcome);
    }
    if let ControlFlow::Break(outcome) = run.retry_execution().await? {
        return Ok(outcome);
    }
    run.finish().await
}

struct LifecycleRun<'a> {
    mode: LifecycleMode,
    store: &'a Arc<Store>,
    task_id: &'a TaskId,
    args: &'a RunArgs,
    agent_display_name: &'a str,
    effective_dir: Option<&'a String>,
    repo_path: Option<&'a String>,
    runtime_hooks: &'a [hooks::Hook],
    checklist_result: Option<checklist_scan::ChecklistResult>,
    quota_error_message: Option<String>,
}

impl<'a> LifecycleRun<'a> {
    fn new(
        mode: LifecycleMode,
        store: &'a Arc<Store>,
        task_id: &'a TaskId,
        args: &'a RunArgs,
        agent_display_name: &'a str,
        effective_dir: Option<&'a String>,
        repo_path: Option<&'a String>,
        runtime_hooks: &'a [hooks::Hook],
    ) -> Self {
        Self {
            mode,
            store,
            task_id,
            args,
            agent_display_name,
            effective_dir,
            repo_path,
            runtime_hooks,
            checklist_result: None,
            quota_error_message: None,
        }
    }

    async fn prepare(
        &mut self,
        wt_path: Option<&String>,
        pre_task_dirty_paths: Option<&[String]>,
        container_name: Option<&str>,
        agent_kind: AgentKind,
        prompt_bundle: &run_prompt::PromptBundle,
    ) -> Result<ControlFlow<Option<TaskId>>> {
        if let ControlFlow::Break(outcome) =
            self.settle_capacity(wt_path, pre_task_dirty_paths).await?
        {
            return Ok(ControlFlow::Break(outcome));
        }
        if let ControlFlow::Break(outcome) = self.settle_worktree(pre_task_dirty_paths).await? {
            return Ok(ControlFlow::Break(outcome));
        }
        self.verify_postprocess(container_name, agent_kind, prompt_bundle)
            .await
    }

    async fn settle_capacity(
        &mut self,
        wt_path: Option<&String>,
        pre_task_dirty_paths: Option<&[String]>,
    ) -> Result<ControlFlow<Option<TaskId>>> {
        let store = self.store;
        let task_id = self.task_id;
        let args = self.args;
        let effective_dir = self.effective_dir;
        let repo_path = self.repo_path;
        run_teardown_phase(task_id, args, wt_path);
        run_escape_checks_phase(args, effective_dir, repo_path, wt_path);
        if store.is_caller_owned(task_id.as_str())? {
            return Ok(ControlFlow::Continue(()));
        }
        if let Some(task) = store.get_task(task_id.as_str())?
            && task.status == TaskStatus::Failed
            && crate::routing::capacity::read(store, task_id, task.agent).is_some()
        {
            if task.verify_status == VerifyStatus::Pending {
                store.update_verify_status(task_id.as_str(), VerifyStatus::Skipped)?;
            }
            if args.read_only
                && run_worktree_settlement_phase(
                    store,
                    task_id,
                    args,
                    effective_dir,
                    pre_task_dirty_paths,
                )
                .await?
                    == LifecyclePhaseDecision::Stop
            {
                return Ok(ControlFlow::Break(None));
            }
            capture_final_worktree_state(store, task_id)?;
            persist_result_file(store, task_id, args, &task, effective_dir);
            return retry_or_cascade(store, args, task_id)
                .await
                .map(ControlFlow::Break);
        }
        Ok(ControlFlow::Continue(()))
    }
    async fn settle_worktree(
        &mut self,
        pre_task_dirty_paths: Option<&[String]>,
    ) -> Result<ControlFlow<Option<TaskId>>> {
        let store = self.store;
        let task_id = self.task_id;
        let args = self.args;
        let effective_dir = self.effective_dir;
        match run_worktree_settlement_phase(
            store,
            task_id,
            args,
            effective_dir,
            pre_task_dirty_paths,
        )
        .await?
        {
            LifecyclePhaseDecision::Continue => {}
            LifecyclePhaseDecision::Retry(retry_id) => {
                record_skipped_configured_verify(
                    store,
                    task_id,
                    args,
                    format!("dirty worktree rescue dispatched retry {retry_id} before verify"),
                );
                return Ok(ControlFlow::Break(Some(retry_id)));
            }
            LifecyclePhaseDecision::Stop => {
                self.settle_stopped()?;
                return Ok(ControlFlow::Break(None));
            }
        }
        Ok(ControlFlow::Continue(()))
    }
    fn settle_stopped(&self) -> Result<()> {
        let store = self.store;
        let task_id = self.task_id;
        let args = self.args;
        let effective_dir = self.effective_dir;
        let repo_path = self.repo_path;
        if args.read_only {
            store.update_verify_status(task_id.as_str(), VerifyStatus::Skipped)?;
            if let Some(task) = store.get_task(task_id.as_str())? {
                persist_result_file(store, task_id, args, &task, effective_dir);
            }
            maybe_run_post_done_audit(
                store,
                task_id,
                args,
                effective_dir.map(String::as_str),
                repo_path.map(String::as_str),
            )?;
        } else {
            record_skipped_configured_verify(
                store,
                task_id,
                args,
                "dirty worktree settlement failed before verify".to_string(),
            );
        }
        Ok(())
    }

    async fn verify_postprocess(
        &mut self,
        container_name: Option<&str>,
        agent_kind: AgentKind,
        prompt_bundle: &run_prompt::PromptBundle,
    ) -> Result<ControlFlow<Option<TaskId>>> {
        run_verify_scope_phase(
            self.store,
            self.task_id,
            self.args,
            self.effective_dir.map(String::as_str),
            container_name,
        );
        self.checklist_result = run_checklist_phase(self.store, self.task_id, self.args)?;
        self.quota_error_message = run_task_postprocess_phase(
            self.store,
            self.task_id,
            self.args,
            agent_kind,
            self.agent_display_name,
            self.effective_dir,
            self.repo_path,
            self.runtime_hooks,
            prompt_bundle,
        )?;
        if let Some(retry_id) = maybe_judge_retry(self.store, self.args, self.task_id).await? {
            return Ok(ControlFlow::Break(Some(retry_id)));
        }
        Ok(ControlFlow::Continue(()))
    }
}

pub(in crate::cmd::run) async fn retry_or_cascade(
    store: &Arc<Store>,
    args: &RunArgs,
    task_id: &TaskId,
) -> Result<Option<TaskId>> {
    if store.is_caller_owned(task_id.as_str())? {
        return Ok(None);
    }
    if let Some(mut retry_args) = retry_logic::prepare_retry(store.clone(), task_id, args).await? {
        if let Some(task) = store.get_task(task_id.as_str())? {
            inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
        }
        return Box::pin(run(store.clone(), retry_args)).await.map(Some);
    } else if let Some(task) = store.get_task(task_id.as_str())?
        && task.status == TaskStatus::Failed
        && let Some((next_agent, remaining_cascade)) = take_next_cascade_agent(args)
    {
        aid_info!(
            "[aid] Cascade: trying {} after {} failed",
            next_agent,
            args.agent_name
        );
        let mut cascade_args = args.clone();
        crate::cmd::run::switch_agent(&mut cascade_args, next_agent);
        cascade_args.cascade = remaining_cascade;
        cascade_args.parent_task_id = Some(task_id.as_str().to_string());
        inherit_cascade_target(&mut cascade_args, &task)?;
        return Box::pin(run(store.clone(), cascade_args)).await.map(Some);
    }
    Ok(None)
}

#[cfg(test)]
#[path = "phases_tests.rs"]
mod tests;

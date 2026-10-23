// Background dispatch phases sharing borrowed dispatch inputs.
// Depends on the parent worker launch and baseline helpers.
use super::*;

pub(super) struct BackgroundRun<'a> {
    store: &'a Arc<Store>,
    args: &'a RunArgs,
    spec: BackgroundRunSpec,
    prepared: &'a PreparedDispatch,
}

impl<'a> BackgroundRun<'a> {
    pub(super) fn prepare(
        store: &'a Arc<Store>,
        args: &'a RunArgs,
        prepared: &'a PreparedDispatch,
        prompt_bundle: &run_prompt::PromptBundle,
    ) -> Result<Self> {
        background::check_worker_capacity(store).or_else(|err| {
            crate::cmd::run::run_dispatch_prepare::record_error(store, &prepared.task_id, &err)?;
            Err(err)
        })?;
        let pre_task_dirty_paths = background_dirty_baseline(store, args, prepared)?;
        let spec = Self::spec(args, prepared, prompt_bundle, pre_task_dirty_paths);
        Ok(Self {
            store,
            args,
            prepared,
            spec,
        })
    }

    fn spec(
        args: &RunArgs,
        prepared: &PreparedDispatch,
        prompt_bundle: &run_prompt::PromptBundle,
        pre_task_dirty_paths: Option<Vec<String>>,
    ) -> BackgroundRunSpec {
        BackgroundRunSpec {
            task_id: prepared.task_id.as_str().to_string(),
            worker_pid: None,
            agent_name: prepared.agent_display_name.clone(),
            prompt: prompt_bundle.effective_prompt.clone(),
            dir: prepared.effective_dir.clone(),
            output: args.output.clone(),
            result_file: args.result_file.clone(),
            result_file_required: args.result_file_required,
            model: prepared.effective_model.clone(),
            budget: prepared.budget_active,
            session_id: args.session_id.clone(),
            verify: args.verify.clone(),
            setup: args.setup.clone(),
            iterate: args.iterate,
            eval: args.eval.clone(),
            eval_feedback_template: args.eval_feedback_template.clone(),
            judge: args.judge.clone(),
            judge_retry: args.judge_retry,
            max_duration_mins: Some(args.timeout_policy.max_duration_mins()),
            max_duration_secs: Some(args.timeout_policy.max_duration.as_secs()),
            idle_timeout_secs: Some(args.timeout_policy.idle.as_secs()),
            max_task_cost: args.max_task_cost,
            retry: args.retry,
            group: args.group.clone(),
            skills: args.skills.clone(),
            checklist: args.checklist.clone(),
            hooks: args.hooks.clone(),
            template: args.template.clone(),
            worktree: args.worktree.clone(),
            base_branch: args.base_branch.clone(),
            peer_review: args.peer_review.clone(),
            audit: args.audit,
            audit_explicit: args.audit_explicit,
            no_audit: args.no_audit,
            scope: args.scope.clone(),
            interactive: true,
            on_done: args.on_done.clone(),
            cascade: args.cascade.clone(),
            parent_task_id: args.parent_task_id.clone(),
            env: args.env.clone(),
            env_forward: args.env_forward.clone(),
            agent_pid: None,
            agent_started: None,
            sandbox: args.sandbox,
            read_only: args.read_only,
            audit_report_mode: args.audit_report_mode,
            container: args.container.clone(),
            link_deps: args.link_deps,
            pre_task_dirty_paths,
            foreground: args.foreground,
        }
    }

    pub(super) fn launch(&self) -> Result<()> {
        let store = self.store;
        let prepared = self.prepared;
        launch_background_worker(store, prepared, &self.spec)?;
        #[cfg(test)]
        {
            let worker_store = Arc::clone(store);
            let worker_task_id = prepared.task_id.as_str().to_string();
            tokio::spawn(async move {
                let _ = background::run_task(worker_store, &worker_task_id).await;
            });
        }
        Ok(())
    }

    pub(super) fn announce(&self) {
        let args = self.args;
        let prepared = self.prepared;
        if args.announce {
            println!(
                "{}",
                crate::cli::dispatch::background_status_line(
                    &prepared.task_id,
                    &prepared.agent_display_name,
                    &args.prompt,
                )
            );
            aid_hint!("[aid] Wait: aid wait {}", prepared.task_id);
        }
    }
}

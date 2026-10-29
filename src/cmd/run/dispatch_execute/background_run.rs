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
    ) -> Result<Self> {
        background::check_worker_capacity(store).or_else(|err| {
            crate::cmd::run::run_dispatch_prepare::record_error(store, &prepared.task_id, &err)?;
            Err(err)
        })?;
        let pre_task_dirty_paths = background_dirty_baseline(store, args, prepared)?;
        let spec = Self::spec(args, prepared, pre_task_dirty_paths);
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
        pre_task_dirty_paths: Option<Vec<String>>,
    ) -> BackgroundRunSpec {
        BackgroundRunSpec {
            task_id: prepared.task_id.as_str().to_string(),
            worker_pid: None,
            dir: prepared.effective_dir.clone(),
            budget: prepared.budget_active,
            max_duration_mins: Some(args.timeout_policy.max_duration_mins()),
            max_duration_secs: Some(args.timeout_policy.max_duration.as_secs()),
            idle_timeout_secs: Some(args.timeout_policy.idle.as_secs()),
            interactive: true,
            on_done: args.on_done.clone(),
            env: args.env.clone(),
            agent_pid: None,
            agent_started: None,
            audit_report_mode: args.audit_report_mode,
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

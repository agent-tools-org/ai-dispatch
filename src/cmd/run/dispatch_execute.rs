// Execution helpers for `aid run` after dispatch setup and prompt assembly.
// Exports: load_runtime_hooks(), run_background_task(), run_foreground_task().
// Deps: hooks, background, container/sandbox wrappers, run lifecycle modules.
#[path = "dispatch_execute/background_run.rs"]
mod background_run;
use background_run::BackgroundRun;

use super::run_dispatch_prepare::PreparedDispatch;
use super::{RunArgs, run_foreground_watch, run_prompt};
use crate::config::hooks;
use crate::runtime::background::{self, BackgroundRunSpec};
use crate::store::Store;
use crate::types::TaskId;
use crate::workspace::commit;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub(super) fn load_runtime_hooks(args: &RunArgs) -> Result<Vec<hooks::Hook>> {
    let mut runtime_hooks = hooks::load_hooks()?;
    runtime_hooks.extend(hooks::parse_cli_hooks(&args.hooks)?);
    Ok(runtime_hooks)
}

pub(super) fn maybe_record_start_sha(
    store: &Arc<Store>,
    task_id: &TaskId,
    effective_dir: Option<&String>,
) -> Result<()> {
    if let Some(dir) = effective_dir
        && let Ok(start_sha) = commit::head_sha(dir)
    {
        store.update_start_sha(task_id.as_str(), &start_sha)?;
    }
    Ok(())
}

fn capture_pre_task_dirty_paths(dir: Option<&String>) -> Option<Vec<String>> {
    let dir = dir?;
    match crate::workspace::worktree::capture_worktree_snapshot(Path::new(dir)) {
        Ok(snapshot) => Some(snapshot.status_lines),
        Err(err) => {
            aid_warn!("[aid] rescue: failed to capture pre-task dirty baseline in {dir}: {err}");
            None
        }
    }
}

pub(super) fn run_background_task(
    store: &Arc<Store>,
    args: &RunArgs,
    prepared: &PreparedDispatch,
    _prompt_bundle: &run_prompt::PromptBundle,
) -> Result<()> {
    let run = BackgroundRun::prepare(store, args, prepared)?;
    run.launch()?;
    run.announce();
    Ok(())
}

pub(super) async fn run_foreground_task(
    store: &Arc<Store>,
    args: &RunArgs,
    prepared: &PreparedDispatch,
    prompt_bundle: &run_prompt::PromptBundle,
) -> Result<Option<TaskId>> {
    if args.announce {
        println!(
            "Task {} started ({}: {})",
            prepared.task_id,
            prepared.agent_display_name,
            crate::agent::truncate::truncate_text(&args.prompt, 50)
        );
    }
    let mut worker_args = args.clone();
    worker_args.announce = false;
    worker_args.foreground = true;
    run_background_task(store, &worker_args, prepared, prompt_bundle)?;
    let final_task_id = run_foreground_watch::wait_for_task(store, &prepared.task_id).await?;
    if final_task_id != prepared.task_id {
        return Ok(Some(final_task_id));
    }
    Ok(None)
}

fn background_dirty_baseline(
    store: &Arc<Store>,
    args: &RunArgs,
    prepared: &PreparedDispatch,
) -> Result<Option<Vec<String>>> {
    let pre_task_dirty_paths = if args.audit_report_mode && !args.read_only {
        None
    } else if args.read_only {
        match super::run_dirty::capture_baseline(
            store,
            &prepared.task_id,
            prepared.effective_dir.as_deref(),
        ) {
            Ok(baseline) => baseline,
            Err(err) => {
                super::run_dispatch_prepare::record_error(store, &prepared.task_id, &err)?;
                return Err(err);
            }
        }
    } else {
        capture_pre_task_dirty_paths(prepared.effective_dir.as_ref())
    };
    Ok(pre_task_dirty_paths)
}

fn spawn_background_worker(store: &Store, spec: &BackgroundRunSpec) -> Result<u32> {
    background::save_spec(spec)?;
    #[cfg(not(test))]
    let spawn = background::spawn_worker(spec.task_id.as_str())?;
    let worker_pid = store.with_worker_launch(spec.task_id.as_str(), || {
        #[cfg(test)]
        let pid = tests::spawn_worker()?;
        #[cfg(not(test))]
        let (launcher, pid) = spawn()?;
        let published = background::update_worker_pid(spec.task_id.as_str(), pid);
        if published.is_err() {
            background::sigkill_process(pid);
        }
        #[cfg(test)]
        {
            Ok((pid, published))
        }
        #[cfg(not(test))]
        {
            Ok((launcher, pid, published))
        }
    })?;
    #[cfg(test)]
    let (worker_pid, published) = worker_pid;
    #[cfg(not(test))]
    let (mut launcher, worker_pid, published) = worker_pid;
    #[cfg(not(test))]
    {
        use std::io::Write;
        let acknowledged = launcher.stdin.take().filter(|_| published.is_ok())
            .map(|mut pipe| pipe.write_all(b"1"));
        std::thread::spawn(move || {
            let _ = launcher.wait();
        });
        if let Some(Err(error)) = acknowledged {
            background::sigkill_process(worker_pid);
            return Err(error.into());
        }
    }
    published?;
    Ok(worker_pid)
}

fn launch_background_worker(
    store: &Arc<Store>,
    prepared: &PreparedDispatch,
    spec: &BackgroundRunSpec,
) -> Result<()> {
    let worker_pid = spawn_background_worker(store, spec);
    let worker_pid = match worker_pid {
        Ok(pid) => pid,
        Err(err) => {
            let _ = background::clear_spec(prepared.task_id.as_str());
            super::run_dispatch_prepare::record_error(store, &prepared.task_id, &err)?;
            run_prompt::notify_task_completion(store, &prepared.task_id)?;
            return Err(err);
        }
    };
    if let Some(wt_path) = prepared.wt_path.as_deref()
        && let Err(holder) = crate::workspace::worktree::rekey_worktree_lock_to_worker(
            Path::new(wt_path),
            prepared.task_id.as_str(),
            worker_pid,
        )
    {
        background::kill_process(worker_pid);
        let _ = background::clear_spec(prepared.task_id.as_str());
        let err = anyhow::anyhow!(
            "Worktree {wt_path} lock is owned by task {holder}; background dispatch aborted"
        );
        super::run_dispatch_prepare::record_error(store, &prepared.task_id, &err)?;
        run_prompt::notify_task_completion(store, &prepared.task_id)?;
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
#[path = "dispatch_execute_tests.rs"]
mod tests;

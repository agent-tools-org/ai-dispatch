// Piped worker spawn and autonomous absolute-deadline enforcement.
// Watches real agent streams within min(max_duration, hard_cap), then settles timeout failure.
// Deps: RunProcessArgs, shared watchers, process-group cleanup and execution events.

use super::*;
use std::time::Instant;
use tokio::process::Child;

pub(super) fn spawn_agent(args: &mut RunProcessArgs<'_>) -> Result<Child> {
    #[cfg(unix)]
    args.cmd.process_group(0);
    crate::cmd::noninteractive_stdio::configure(&mut args.cmd);
    let child = match args.cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            let err = anyhow::Error::new(err).context("Failed to spawn agent process");
            let stderr = stderr_excerpt(args.task_id)
                .or_else(|| Some("unavailable (process did not start)".to_string()));
            fail_task_on_agent_spawn(args.store, args.task_id, &err, stderr.as_deref());
            return Err(err);
        }
    };
    if let Some(pid) = child.id() {
        let _ = crate::background::update_agent_pid(args.task_id.as_str(), pid);
    }
    Ok(child)
}

pub(super) async fn watch_agent(
    args: &RunProcessArgs<'_>,
    child: &mut Child,
    start: Instant,
) -> Result<CompletionInfo> {
    let limit = args.timeout_policy.max_duration.min(args.timeout_policy.hard_cap);
    let watch = async {
        if args.streaming {
            crate::watcher::watch_streaming(
                args.agent,
                child,
                args.task_id,
                args.store,
                args.log_path,
                args.workgroup_id,
                args.timeout_policy.idle,
                args.max_task_cost,
            )
            .await
        } else {
            crate::watcher::watch_buffered(
                args.agent,
                child,
                args.task_id,
                args.store,
                args.log_path,
                args.output_path.map(std::path::Path::new),
                args.workgroup_id,
            )
            .await
        }
    };
    let result = tokio::time::timeout(limit.saturating_sub(start.elapsed()), watch).await;
    settle_result(args, child, result).await
}

async fn settle_result(
    args: &RunProcessArgs<'_>,
    child: &mut Child,
    result: std::result::Result<Result<CompletionInfo>, tokio::time::error::Elapsed>,
) -> Result<CompletionInfo> {
    let limit = args.timeout_policy.max_duration.min(args.timeout_policy.hard_cap);
    match result {
        Ok(Ok(info)) => Ok(info),
        Ok(Err(err)) => {
            crate::process_group::force_kill_process_group(child);
            let _ = child.kill().await;
            let _ = child.wait().await;
            let stderr = stderr_excerpt(args.task_id);
            insert_phase_error_event(
                args.store,
                args.task_id,
                "execution",
                &err.to_string(),
                stderr.as_deref(),
            );
            Err(err)
        }
        Err(_) => {
            crate::process_group::force_kill_process_group(child);
            insert_phase_error_event(
                args.store,
                args.task_id,
                "execution",
                &format!("exceeded hard timeout of {} seconds", limit.as_secs()),
                None,
            );
            Ok(CompletionInfo {
                status: TaskStatus::Failed,
                tokens: None,
                model: None,
                cost_usd: None,
                exit_code: None,
            })
        }
    }
}

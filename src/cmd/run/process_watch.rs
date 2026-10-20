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
    let mut child = match args.store.with_running_task(args.task_id.as_str(), || {
        let child = args.cmd.spawn()?;
        if let Some(pid) = child.id() {
            let _ = crate::runtime::background::update_agent_pid(args.task_id.as_str(), pid);
        }
        Ok(child)
    }) {
        Ok(child) => child,
        Err(err) => {
            let err = err.context("Failed to spawn agent process");
            let stderr = stderr_excerpt(args.task_id)
                .or_else(|| Some("unavailable (process did not start)".to_string()));
            fail_task_on_agent_spawn(args.store, args.task_id, &err, stderr.as_deref());
            return Err(err);
        }
    };
    if let Err(error) = crate::runtime::read_only_launch::record(
        args.store, args.task_id, args.cmd.as_std(), "exec",
    ) {
        cleanup_process_group(&child);
        let _ = child.start_kill();
        return Err(error);
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
            crate::runtime::watcher::watch_streaming(
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
            crate::runtime::watcher::watch_buffered(
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
    settle_result(args, child, start, result).await
}

async fn settle_result(
    args: &RunProcessArgs<'_>,
    child: &mut Child,
    start: Instant,
    result: std::result::Result<Result<CompletionInfo>, tokio::time::error::Elapsed>,
) -> Result<CompletionInfo> {
    let limit = args.timeout_policy.max_duration.min(args.timeout_policy.hard_cap);
    let duration_ms = start.elapsed().as_millis() as i64;
    match result {
        Ok(Ok(info)) => Ok(info),
        Ok(Err(err)) => {
            crate::runtime::process::group::force_kill_process_group(child);
            let _ = child.kill().await;
            let _ = child.wait().await;
            settle_execution_failure(args, duration_ms, &err.to_string());
            Err(err)
        }
        Err(_) => {
            crate::runtime::process::group::force_kill_process_group(child);
            settle_execution_failure(
                args, duration_ms, &format!("exceeded hard timeout of {} seconds", limit.as_secs()),
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

fn settle_execution_failure(args: &RunProcessArgs<'_>, duration_ms: i64, detail: &str) {
    let event = TaskEvent {
        task_id: args.task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: format!("Failed during execution: {detail}"),
        metadata: None,
    };
    let _ = crate::task::lifecycle::complete_task_atomic(
        args.store,
        TaskCompletionUpdate {
            id: args.task_id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms,
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: None,
        },
        &event,
    );
}

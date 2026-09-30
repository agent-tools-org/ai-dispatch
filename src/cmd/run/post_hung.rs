// Hung-run retry planning and dispatch after lifecycle completion.
// Reuses persisted retry configuration and overlays the worker runtime.
use anyhow::Result;
use std::sync::Arc;
use crate::{process_monitor, store::Store, types::*};
use crate::cmd::{retry_logic, run_hung_recovery};
use super::{switch_agent, take_next_cascade_agent};
use super::super::{RunArgs, inherit_retry_base_branch, run, apply_retry_target};

pub(crate) async fn maybe_auto_retry_after_hang(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }

    let events = store.get_events(task_id.as_str())?;
    let Some(context) = process_monitor::hung_context(&events) else {
        return Ok(None);
    };
    let retry_count = prior_hung_retry_count(store.as_ref(), &task)?;
    let Some(retries_left) = hung_retry_retries_left(args, &context, retry_count) else {
        return Ok(None);
    };
    let hung_task = run_hung_recovery::with_hung_context(&task, &context);
    if !run_hung_recovery::should_auto_retry_hung(&task, &context, retry_count) {
        return Ok(None);
    }

    aid_warn!(
        "[aid] Agent hung, auto-retrying ({} retries left)",
        retries_left
    );

    let feedback =
        run_hung_recovery::build_hung_retry_feedback(&hung_task, context.hung_duration_secs);
    let root_prompt = retry_logic::root_prompt(store.as_ref(), &task)
        .unwrap_or_else(|| args.prompt.clone());
    let retry_args = build_hung_retry_args(store, args, &task, &context, &feedback, &root_prompt)?;

    process_monitor::insert_hung_retry_event(store.as_ref(), task_id)?;
    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    Ok(Some(retry_id))
}

fn hung_retry_retries_left(
    args: &RunArgs,
    context: &process_monitor::HungContext,
    retry_count: u32,
) -> Option<u32> {
    if context.transient {
        return Some(
            run_hung_recovery::MAX_TRANSIENT_HUNG_RETRIES
                .saturating_sub(retry_count)
                .saturating_sub(1),
        );
    }
    if args.retry == 0 {
        return None;
    }
    Some(args.retry.saturating_sub(1))
}

fn build_hung_retry_args(
    store: &Store,
    args: &RunArgs,
    task: &Task,
    context: &process_monitor::HungContext,
    feedback: &str,
    root_prompt: &str,
) -> Result<RunArgs> {
    let mut retry_args = RunArgs::for_retry(store, task)?;
    retry_args.dir = args.dir.clone();
    retry_args.env = args.env.clone();
    retry_args.foreground = args.foreground;
    retry_args.announce = args.announce;
    retry_args.on_done = None;
    retry_args.prompt =
        format!("[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{root_prompt}");
    retry_args.retry = if context.transient {
        args.retry
    } else {
        args.retry.saturating_sub(1)
    };
    retry_args.parent_task_id = Some(task.id.as_str().to_string());
    retry_args.background = false;
    apply_retry_target(task, &mut retry_args)?;
    inherit_retry_base_branch(args.dir.as_deref(), task, &mut retry_args);
    if context.transient {
        retry_args.session_id = None;
        if let Some((next_agent, remaining_cascade)) = take_next_cascade_agent(args) {
            // A model name means something only inside one CLI. Carrying the
            // parent's across a cascade sent agy `gpt-5.6-luna` — codex's model
            // — and agy refused it by listing its own (`t-ac9a7a9d`, cascaded
            // from `t-90371f9e`). The cascade exists to escape a failing route;
            // inheriting half of that route defeats the point.
            //
            // Dropped only when the agent actually changes. A same-agent retry
            // must still ask for what was asked before.
            switch_agent(&mut retry_args, next_agent);
            retry_args.cascade = remaining_cascade;
        }
    }
    Ok(retry_args)
}


fn prior_hung_retry_count(store: &Store, task: &Task) -> Result<u32> {
    let chain = store.get_retry_chain(task.id.as_str())?;
    Ok(chain
        .into_iter()
        .filter(|entry| entry.id != task.id)
        .filter_map(|entry| store.get_events(entry.id.as_str()).ok())
        .filter(|events| process_monitor::was_auto_retried_after_hang(events))
        .count() as u32)
}

#[cfg(test)]
#[path = "post_tests.rs"]
mod tests;

// Iteration loop helpers for `aid run --iterate --eval`.
// Exports iterate config parsing, eval execution, and iterate retry dispatch.
// Deps: run args/wrappers, store task history, and std process execution.
use anyhow::{Context, Result, bail};
use chrono::Local;
use serde_json::json;
use std::process::Command;
use std::sync::Arc;

use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus};

use super::RunArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IterateConfig {
    pub max_iterations: u32,
    pub eval_command: String,
    pub feedback_template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvalResult {
    exit_code: i32,
    output: String,
}

pub(crate) fn iterate_config(args: &RunArgs) -> Result<Option<IterateConfig>> {
    match (args.iterate, args.eval.as_deref(), args.eval_feedback_template.as_deref()) {
        (None, None, None) => Ok(None),
        (Some(0), _, _) => bail!("--iterate must be at least 1"),
        (Some(_), None, _) => bail!("--iterate requires --eval"),
        (None, Some(_), _) => bail!("--eval requires --iterate"),
        (None, None, Some(_)) => bail!("--eval-feedback-template requires --iterate"),
        (Some(max_iterations), Some(eval_command), feedback_template) => {
            let eval_command = eval_command.trim();
            if eval_command.is_empty() {
                bail!("--eval cannot be empty");
            }
            Ok(Some(IterateConfig {
                max_iterations,
                eval_command: eval_command.to_string(),
                feedback_template: feedback_template.map(ToString::to_string),
            }))
        }
    }
}

pub async fn maybe_iterate(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    iterate_config: &IterateConfig,
) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    if task.status != TaskStatus::Done {
        return Ok(None);
    }

    let iteration = iteration_for_task(store.as_ref(), &task)?;
    let working_dir = args
        .dir
        .as_deref()
        .or(task.worktree_path.as_deref())
        .or(task.repo_path.as_deref())
        .unwrap_or(".");
    let eval_result = run_eval_command(&iterate_config.eval_command, working_dir)?;

    if eval_result.exit_code == 0 {
        insert_iteration_event(
            store.as_ref(),
            task_id,
            format!("Iteration {iteration}/{}: eval passed", iterate_config.max_iterations),
            iteration,
            iterate_config.max_iterations,
            "passed",
            None,
        );
        return Ok(None);
    }

    if iteration >= iterate_config.max_iterations {
        insert_iteration_event(
            store.as_ref(),
            task_id,
            format!(
                "Iteration {iteration}/{}: eval failed (exit {}), max iterations reached",
                iterate_config.max_iterations, eval_result.exit_code
            ),
            iteration,
            iterate_config.max_iterations,
            "max_reached",
            Some(&eval_result.output),
        );
        return Ok(None);
    }

    let next_iteration = iteration + 1;
    let root_prompt =
        crate::cmd::retry_logic::root_prompt(store.as_ref(), &task).unwrap_or_else(|| args.prompt.clone());
    let feedback = build_feedback_prompt(
        iterate_config.feedback_template.as_deref(),
        next_iteration,
        iterate_config.max_iterations,
        &eval_result.output,
    );
    let retry_task_id = TaskId::generate();
    insert_iteration_event(
        store.as_ref(),
        task_id,
        format!(
            "Iteration {iteration}/{}: eval failed (exit {}), retrying as {}",
            iterate_config.max_iterations, eval_result.exit_code, retry_task_id
        ),
        iteration,
        iterate_config.max_iterations,
        "retrying",
        Some(&eval_result.output),
    );

    let mut retry_args = args.clone();
    retry_args.prompt = format!("[Iteration feedback]\n{feedback}\n\n[Original task]\n{root_prompt}");
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.background = false;
    retry_args.existing_task_id = Some(retry_task_id.clone());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task.output_path.clone().or_else(|| retry_args.output.clone());
    retry_args.model = task.model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.budget = task.budget;
    let (dir, worktree) = super::retry_target(&task);
    retry_args.dir = dir.or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    if task.agent == AgentKind::OpenCode {
        retry_args.session_id = task.agent_session_id.clone();
    }

    let final_task_id = Box::pin(super::run(store.clone(), retry_args)).await?;
    insert_iteration_event(
        store.as_ref(),
        &retry_task_id,
        format!("Iteration {next_iteration}/{}", iterate_config.max_iterations),
        next_iteration,
        iterate_config.max_iterations,
        "scheduled",
        None,
    );
    Ok(Some(final_task_id))
}

fn run_eval_command(eval_cmd: &str, working_dir: &str) -> Result<EvalResult> {
    let output = Command::new("sh")
        .args(["-lc", eval_cmd])
        .current_dir(working_dir)
        .output()
        .with_context(|| format!("failed to run eval command in {working_dir}: {eval_cmd}"))?;
    Ok(EvalResult {
        exit_code: output.status.code().unwrap_or(-1),
        output: merge_eval_output(&output.stdout, &output.stderr),
    })
}

fn merge_eval_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "(no output)".to_string(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn build_feedback_prompt(
    template: Option<&str>,
    iteration: u32,
    max_iterations: u32,
    eval_output: &str,
) -> String {
    let eval_output = if eval_output.trim().is_empty() {
        "(no output)"
    } else {
        eval_output
    };
    match template {
        Some(template) => template
            .replace("{eval_output}", eval_output)
            .replace("{iteration}", &iteration.to_string())
            .replace("{max_iterations}", &max_iterations.to_string()),
        None => format!(
            "Iteration {iteration}/{max_iterations}: eval failed.\nEval output:\n{eval_output}\n\nFix the issues."
        ),
    }
}

fn iteration_for_task(store: &Store, task: &Task) -> Result<u32> {
    let mut current_id = Some(task.id.as_str().to_string());
    while let Some(task_id) = current_id {
        if let Some(iteration) = iteration_from_events(store, &task_id)? {
            return Ok(iteration);
        }
        current_id = store.get_task(&task_id)?.and_then(|entry| entry.parent_task_id);
    }
    Ok(1)
}

fn iteration_from_events(store: &Store, task_id: &str) -> Result<Option<u32>> {
    let events = store.get_events(task_id)?;
    Ok(events.into_iter().rev().find_map(|event| {
        event
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("iterate"))
            .and_then(|metadata| metadata.get("iteration"))
            .and_then(|value| value.as_u64())
            .map(|value| value as u32)
    }))
}

fn insert_iteration_event(
    store: &Store,
    task_id: &TaskId,
    detail: String,
    iteration: u32,
    max_iterations: u32,
    status: &str,
    eval_output: Option<&str>,
) {
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: Some(json!({
            "iterate": {
                "iteration": iteration,
                "max_iterations": max_iterations,
                "status": status,
                "eval_output": eval_output,
            }
        })),
    });
}

#[cfg(test)]
#[path = "run_iterate_tests.rs"]
mod tests;

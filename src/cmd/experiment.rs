// Core experiment loop: dispatch → measure → keep/revert.
// Exports: run_experiment().
// Deps: experiment_types, experiment_persist, cmd::run, store.
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use crate::cmd::experiment_persist::{experiment_file_path, load_state, save_run};
use crate::cmd::experiment_types::{ExperimentConfig, ExperimentRun, ExperimentState};
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::Task;
pub async fn run_experiment(store: Arc<Store>, config: ExperimentConfig) -> Result<()> {
    let base_dir = std::env::current_dir()?.to_string_lossy().to_string();
    let state_path = experiment_file_path(&base_dir);
    let mut state = load_state(&state_path, &config)?;
    let max_runs = config.max_runs.unwrap_or(1);
    for run_id in 1..=max_runs {
        git_snapshot(&base_dir)?;
        let start = Instant::now();
        let task_id = match run::run(store.clone(), RunArgs {
            agent_name: config.agent.clone(),
            prompt: config.prompt.clone(),
            verify: config.verify.clone(),
            worktree: config.worktree.clone(),
            announce: false,
            ..Default::default()
        })
        .await
        {
            Ok(id) => id,
            Err(err) => {
                let _ = git_revert(&base_dir);
                return Err(err.context("experiment dispatch failed"));
            }
        };

        let task = store
            .get_task(task_id.as_str())?
            .context("missing task for experiment run")?;
        let dir = work_dir(&task);

        if let Some(ref checks_cmd) = config.checks && !run_check(checks_cmd, &dir)? {
            git_revert(&dir)?;
            let run = make_run(run_id, &task, None, Some(false), false, start.elapsed().as_millis() as i64);
            persist_run(&mut state, &state_path, run)?;
            aid_warn!("[aid experiment] run {run_id}: checks failed, reverted");
            continue;
        }
        let metric_value = evaluate_metric(&config.metric_command, &dir).with_context(|| "metric command failed")?;
        let kept = state.is_improvement(metric_value);
        if kept {
            git_commit(&dir, &format!("experiment run {run_id}: {metric_value}"))?;
        } else {
            git_revert(&dir)?;
        }

        let run = make_run(run_id, &task, Some(metric_value), config.checks.as_ref().map(|_| true), kept, start.elapsed().as_millis() as i64);
        persist_run(&mut state, &state_path, run)?;
        aid_info!("[aid experiment] run {run_id}: metric={metric_value} kept={kept}");
    }
    Ok(())
}

fn make_run(
    run_id: usize,
    task: &Task,
    metric_value: Option<f64>,
    checks_passed: Option<bool>,
    kept: bool,
    duration_ms: i64,
) -> ExperimentRun {
    ExperimentRun {
        run_id,
        task_id: task.id.to_string(),
        agent: task.agent_display_name().to_string(),
        metric_value,
        checks_passed,
        kept,
        timestamp: Utc::now().to_rfc3339(),
        duration_ms: Some(duration_ms),
    }
}
fn persist_run(state: &mut ExperimentState, path: &Path, run: ExperimentRun) -> Result<()> {
    state.record_run(run.clone());
    save_run(path, &run)
}
fn work_dir(task: &Task) -> String {
    task
        .worktree_path
        .as_deref()
        .or(task.repo_path.as_deref())
        .unwrap_or(".")
        .to_string()
}
fn run_check(command: &str, dir: &str) -> Result<bool> {
    let output = Command::new("sh")
        .args(["-c", command])
        .current_dir(dir)
        .output()?;
    Ok(output.status.success())
}
fn evaluate_metric(cmd: &str, dir: &str) -> Result<f64> {
    let output = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(dir)
        .output()
        .context("metric command failed")?;
    if !output.status.success() {
        anyhow::bail!("metric command exited with {}", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .last()
            .and_then(|line| line.split_whitespace().last())
        .and_then(|word| word.parse::<f64>().ok())
        .ok_or_else(|| anyhow::anyhow!("metric command output is not a number"))
}
fn git_snapshot(dir: &str) -> Result<()> {
    Command::new("git")
        .args(["stash", "create"])
        .current_dir(dir)
        .output()?;
    Ok(())
}
fn git_revert(dir: &str) -> Result<()> {
    Command::new("git")
        .args(["checkout", "."])
        .current_dir(dir)
        .output()?;
    Ok(())
}
fn git_commit(dir: &str, message: &str) -> Result<()> {
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .output()?;
    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(dir)
        .output()?;
    Ok(())
}

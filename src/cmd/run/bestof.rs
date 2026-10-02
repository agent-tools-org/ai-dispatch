// Best-of-N dispatch: race N launchable advice candidates, pick best result.
// Exports: run_best_of(). Deps: run::RunArgs, judge, agent selection.
use anyhow::{anyhow, bail, Result};
use std::cmp::Ordering;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use crate::agent::selection::AdviceCandidate;
use crate::cmd::judge;
use crate::sanitize::{is_valid_task_id, validate_task_id};
use crate::store::Store;
use crate::types::*;
use super::run_validate::{IdConflict, resolve_id_conflict};
use super::{run, RunArgs};
#[path = "bestof/output_files.rs"]
mod output_files;
use self::output_files::{
    DispatchArtifacts, dispatch_artifacts_for_candidate, finalize_winner_artifacts,
};

#[path = "bestof_plan.rs"]
mod plan;
use plan::{advised_plan, racer_args};

struct BestOfDispatch {
    agent_hint: String,
    task_id: TaskId,
}

#[derive(Clone)]
struct CandidateResult {
    task_id: TaskId,
    agent_label: String,
    status: TaskStatus,
    is_success: bool,
    diff_line_count: usize,
    metric_score: Option<f64>,
}

fn evaluate_metric(
    metric_cmd: &str,
    worktree_path: Option<&str>,
    repo_path: Option<&str>,
) -> Option<f64> {
    let mut dirs = Vec::new();
    if let Some(worktree_path) = worktree_path {
        dirs.push(worktree_path);
    }
    if let Some(repo_path) = repo_path
        && !dirs.contains(&repo_path)
    {
        dirs.push(repo_path);
    }
    if dirs.is_empty() {
        dirs.push(".");
    }
    for dir in dirs {
        let Ok(output) = Command::new("sh")
            .args(["-c", metric_cmd])
            .current_dir(dir)
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(score) = stdout
            .trim()
            .lines()
            .last()
            .and_then(|line| line.split_whitespace().last().and_then(|word| word.parse::<f64>().ok()))
            .filter(|score| score.is_finite())
        {
            return Some(score);
        }
    }
    None
}

fn is_completed_best_of_status(status: &TaskStatus) -> bool {
    status.is_terminal() || *status == TaskStatus::AwaitingInput
}

impl CandidateResult {
    fn from_task(task: Task, metric_cmd: Option<&str>) -> Self {
        let agent_label = task
            .custom_agent_name
            .clone()
            .unwrap_or_else(|| task.agent.as_str().to_string());
        let is_success = task.outcome().is_success();
        let diff_line_count = if is_success {
            judge::gather_diff(&task)
                .or_else(|| judge::read_output(&task))
                .map(|text| text.lines().count())
                .unwrap_or(0)
        } else {
            0
        };
        let metric_score = if is_success {
            metric_cmd.and_then(|cmd| {
                evaluate_metric(cmd, task.worktree_path.as_deref(), task.repo_path.as_deref())
            })
        } else {
            None
        };
        CandidateResult {
            task_id: task.id.clone(),
            agent_label,
            status: task.status,
            is_success,
            diff_line_count,
            metric_score,
        }
    }
}

fn pick_best_result(candidates: &[CandidateResult]) -> Option<&CandidateResult> {
    candidates
        .iter()
        .filter(|c| c.is_success)
        .max_by(|a, b| match (
            a.metric_score.filter(|score| score.is_finite()),
            b.metric_score.filter(|score| score.is_finite()),
        ) {
            (Some(sa), Some(sb)) => sa.partial_cmp(&sb).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => a.diff_line_count.cmp(&b.diff_line_count),
        })
}

fn validate_best_of_count(n: usize) -> Result<()> {
    if (2..=5).contains(&n) {
        Ok(())
    } else {
        bail!("--best-of must be between 2 and 5");
    }
}

fn best_of_task_id(
    store: &Store,
    base: Option<&TaskId>,
    candidate_idx: usize,
) -> Result<Option<TaskId>> {
    let Some(base) = base else {
        return Ok(None);
    };
    validate_task_id(base.as_str())?;
    let suffix = format!("-bo{}", candidate_idx + 1);
    let max_base_len = 64usize.saturating_sub(suffix.len());
    let prefix: String = base.as_str().chars().take(max_base_len).collect();
    let derived = format!("{prefix}{suffix}");
    validate_task_id(&derived)?;
    match resolve_id_conflict(store, &derived)? {
        IdConflict::None | IdConflict::ReplaceWaiting => Ok(Some(TaskId(derived))),
        IdConflict::AutoSuffix(new_id) if is_valid_task_id(&new_id) => Ok(Some(TaskId(new_id))),
        IdConflict::AutoSuffix(_) => Ok(None),
        IdConflict::Running => Ok(None),
    }
}

pub async fn run_best_of(store: Arc<Store>, args: RunArgs, n: usize) -> Result<TaskId> {
    validate_best_of_count(n)?;
    let original_artifacts =
        dispatch_artifacts_for_candidate(args.output.as_deref(), args.result_file.as_deref(), 0);
    let (declared, plan) = advised_plan(&store, &args, n)?;
    let (dispatches, candidate_artifacts) = launch_candidates(&store, &args, declared, plan, n).await?;
    let completed = collect_results(&store, dispatches, args.metric.as_deref(), n).await?;
    let best = pick_best_result(&completed)
        .ok_or_else(|| anyhow!("best-of-{n}: no successful tasks"))?;
    finalize_winner_artifacts(&original_artifacts, &candidate_artifacts, &best.task_id)?;
    announce_winner(&completed, best, n);
    Ok(best.task_id.clone())
}

async fn launch_candidates(
    store: &Arc<Store>, args: &RunArgs, declared: DeclaredTaskProfile,
    plan: Vec<AdviceCandidate>, n: usize,
) -> Result<(Vec<BestOfDispatch>, Vec<(TaskId, DispatchArtifacts)>)> {
    let mut dispatches = Vec::new();
    let mut candidate_artifacts = Vec::new();
    for (candidate_idx, candidate) in plan.into_iter().enumerate() {
        let agent_label = candidate.agent.clone();
        let mut child_args = racer_args(args, &candidate, declared);
        let artifacts = dispatch_artifacts_for_candidate(
            args.output.as_deref(),
            args.result_file.as_deref(),
            candidate_idx,
        );
        child_args.output = artifacts.output.clone();
        child_args.result_file = artifacts.result_file.clone();
        child_args.existing_task_id =
            best_of_task_id(store.as_ref(), args.existing_task_id.as_ref(), candidate_idx)?;
        let store = store.clone();
        let dispatch = async {
            if let (Some(kind), Some(model)) = (candidate.kind(), candidate.model.as_deref())
                && !crate::agent::model_validation::validate_model_for_agent(
                    crate::agent::get_agent(kind).as_ref(), model, child_args.model_source,
                )? {
                bail!("advised model '{model}' unavailable; refusing a different default");
            }
            run(store, child_args).await
        }.await;
        match dispatch {
            Ok(task_id) => {
                candidate_artifacts.push((task_id.clone(), artifacts));
                dispatches.push(BestOfDispatch {
                    agent_hint: agent_label,
                    task_id,
                });
            }
            Err(err) => {
                aid_error!("[aid] best-of-{n}: dispatch to {agent_label} failed: {err}");
            }
        }
    }
    if dispatches.is_empty() {
        bail!("best-of-{n}: all dispatch attempts failed");
    }
    Ok((dispatches, candidate_artifacts))
}

async fn collect_results(
    store: &Store, mut pending: Vec<BestOfDispatch>, metric: Option<&str>, n: usize,
) -> Result<Vec<CandidateResult>> {
    let mut completed = Vec::new();
    while !pending.is_empty() {
        let mut done = Vec::new();
        for (idx, dispatch) in pending.iter().enumerate() {
            if let Some(task) = store.get_task(dispatch.task_id.as_str())? {
                if is_completed_best_of_status(&task.status) {
                    done.push(idx);
                    completed.push(CandidateResult::from_task(task, metric));
                }
            } else {
                aid_warn!(
                    "[aid] best-of-{n}: task {} (agent {}) missing from store",
                    dispatch.task_id, dispatch.agent_hint
                );
                done.push(idx);
            }
        }
        for idx in done.into_iter().rev() {
            pending.remove(idx);
        }
        if !pending.is_empty() {
            sleep(Duration::from_secs(2)).await;
        }
    }
    Ok(completed)
}

fn announce_winner(completed: &[CandidateResult], best: &CandidateResult, n: usize) {
    let others = completed
        .iter()
        .filter(|candidate| candidate.task_id != best.task_id)
        .map(|candidate| format!("{} ({})", candidate.task_id, candidate.agent_label))
        .collect::<Vec<_>>()
        .join(", ");
    if others.is_empty() {
        println!(
            "[aid] best-of-{n}: picked {} ({})",
            best.task_id, best.agent_label
        );
    } else {
        println!(
            "[aid] best-of-{n}: picked {} ({}) over {}",
            best.task_id, best.agent_label, others
        );
    }
}

#[cfg(test)]
#[path = "bestof/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "bestof/additional_tests.rs"]
mod additional_tests;

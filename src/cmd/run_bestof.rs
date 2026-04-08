// Best-of-N dispatch: send task to N budget agents, pick best result.
// Exports: run_best_of(). Deps: run::RunArgs, judge, agent selection.
use anyhow::{anyhow, bail, Result};
use std::cmp::Ordering;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use crate::agent::{self, RunOpts};
use crate::cmd::judge;
use crate::sanitize::{is_valid_task_id, validate_task_id};
use crate::store::Store;
use crate::team;
use crate::types::*;
use super::run_validate::{IdConflict, resolve_id_conflict};
use super::{run, RunArgs};
#[path = "run_bestof/output_files.rs"]
mod output_files;
use self::output_files::{
    dispatch_artifacts_for_candidate, finalize_winner_artifacts,
};

struct BestOfDispatch {
    agent_hint: String,
    task_id: TaskId,
}

#[derive(Clone)]
struct CandidateResult {
    task_id: TaskId,
    agent_label: String,
    status: TaskStatus,
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

fn is_success_status(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done | TaskStatus::Merged)
}

fn is_completed_best_of_status(status: &TaskStatus) -> bool {
    status.is_terminal() || *status == TaskStatus::AwaitingInput
}

fn expand_best_of_plan(mut plan: Vec<AgentKind>, n: usize) -> Vec<AgentKind> {
    let base_len = plan.len();
    while plan.len() < n {
        plan.push(plan[plan.len() % base_len]);
    }
    plan
}

impl CandidateResult {
    fn from_task(task: Task, metric_cmd: Option<&str>) -> Self {
        let agent_label = task
            .custom_agent_name
            .clone()
            .unwrap_or_else(|| task.agent.as_str().to_string());
        let diff_line_count = if is_success_status(&task.status) {
            judge::gather_diff(&task)
                .or_else(|| judge::read_output(&task))
                .map(|text| text.lines().count())
                .unwrap_or(0)
        } else {
            0
        };
        let metric_score = if is_success_status(&task.status) {
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
            diff_line_count,
            metric_score,
        }
    }
}

fn pick_best_result(candidates: &[CandidateResult]) -> Option<&CandidateResult> {
    candidates
        .iter()
        .filter(|c| is_success_status(&c.status))
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
    if candidate_idx == 0 {
        return Ok(Some(base.clone()));
    }
    match resolve_id_conflict(store, base.as_str())? {
        IdConflict::None | IdConflict::ReplaceWaiting => return Ok(Some(base.clone())),
        IdConflict::Running | IdConflict::AutoSuffix(_) => {}
    }
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
    let team_config = args.team.as_deref().and_then(team::resolve_team);
    let selection_opts = RunOpts {
        dir: args.dir.clone(),
        output: args.output.clone(),
        result_file: args.result_file.clone(),
        model: args.model.clone(),
        budget: true,
        read_only: args.read_only,
        context_files: Vec::new(),
        session_id: args.session_id.clone(),
        env: None,
        env_forward: None,
    };
    let agents = agent::selection::budget_ranked_agents(
        &args.prompt,
        &selection_opts,
        &store,
        team_config.as_ref(),
    );
    if agents.is_empty() {
        bail!("best-of-{n}: no budget agents available");
    }
    let plan = expand_best_of_plan(agents.into_iter().take(n).collect(), n);
    let mut dispatches = Vec::new();
    let mut candidate_artifacts = Vec::new();
    for (candidate_idx, kind) in plan.into_iter().enumerate() {
        let agent_label = kind.as_str().to_string();
        let mut child_args = args.clone();
        let artifacts = dispatch_artifacts_for_candidate(
            args.output.as_deref(),
            args.result_file.as_deref(),
            candidate_idx,
        );
        child_args.agent_name = agent_label.clone();
        child_args.background = true;
        child_args.judge = None;
        child_args.announce = false;
        child_args.best_of = None;
        child_args.output = artifacts.output.clone();
        child_args.result_file = artifacts.result_file.clone();
        child_args.existing_task_id =
            best_of_task_id(store.as_ref(), args.existing_task_id.as_ref(), candidate_idx)?;
        let store = store.clone();
        match run(store, child_args).await {
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
    let mut pending = dispatches;
    let mut completed = Vec::new();
    while !pending.is_empty() {
        let mut done = Vec::new();
        for (idx, dispatch) in pending.iter().enumerate() {
            if let Some(task) = store.get_task(dispatch.task_id.as_str())? {
                if is_completed_best_of_status(&task.status) {
                    done.push(idx);
                    completed.push(CandidateResult::from_task(task, args.metric.as_deref()));
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
    let best = pick_best_result(&completed)
        .ok_or_else(|| anyhow!("best-of-{n}: no successful tasks"))?;
    finalize_winner_artifacts(&original_artifacts, &candidate_artifacts, &best.task_id)?;
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
    Ok(best.task_id.clone())
}

#[cfg(test)]
#[path = "run_bestof/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "run_bestof/additional_tests.rs"]
mod additional_tests;

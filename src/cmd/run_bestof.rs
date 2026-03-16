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
use crate::store::Store;
use crate::team;
use crate::types::*;
use super::{run, RunArgs};

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

fn evaluate_metric(metric_cmd: &str, worktree_path: Option<&str>) -> Option<f64> {
    let dir = worktree_path.unwrap_or(".");
    let output = Command::new("sh")
        .args(["-c", metric_cmd])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().last().and_then(|word| word.parse::<f64>().ok()))
}

impl CandidateResult {
    fn from_task(task: Task, metric_cmd: Option<&str>) -> Self {
        let agent_label = task
            .custom_agent_name
            .clone()
            .unwrap_or_else(|| task.agent.as_str().to_string());
        let diff_line_count = if task.status == TaskStatus::Done {
            judge::gather_diff(&task)
                .or_else(|| judge::read_output(&task))
                .map(|text| text.lines().count())
                .unwrap_or(0)
        } else {
            0
        };
        let metric_score = if task.status == TaskStatus::Done {
            metric_cmd.and_then(|cmd| evaluate_metric(cmd, task.worktree_path.as_deref()))
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
        .filter(|c| c.status == TaskStatus::Done)
        .max_by(|a, b| match (a.metric_score, b.metric_score) {
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

pub async fn run_best_of(store: Arc<Store>, args: RunArgs, n: usize) -> Result<TaskId> {
    validate_best_of_count(n)?;
    let team_config = args.team.as_deref().and_then(team::resolve_team);
    let selection_opts = RunOpts {
        dir: args.dir.clone(),
        output: args.output.clone(),
        model: args.model.clone(),
        budget: true,
        read_only: args.read_only,
        context_files: Vec::new(),
        session_id: args.session_id.clone(),
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
    let mut plan: Vec<AgentKind> = agents.into_iter().take(n).collect();
    while plan.len() < n {
        plan.push(plan[0]);
    }
    let mut dispatches = Vec::new();
    for kind in plan {
        let agent_label = kind.as_str().to_string();
        let mut child_args = args.clone();
        child_args.agent_name = agent_label.clone();
        child_args.background = true;
        child_args.judge = None;
        child_args.announce = false;
        child_args.best_of = None;
        let store = store.clone();
        match run(store, child_args).await {
            Ok(task_id) => dispatches.push(BestOfDispatch {
                agent_hint: agent_label,
                task_id,
            }),
            Err(err) => {
                eprintln!("[aid] best-of-{n}: dispatch to {agent_label} failed: {err}");
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
                if task.status.is_terminal() {
                    done.push(idx);
                    completed.push(CandidateResult::from_task(task, args.metric.as_deref()));
                }
            } else {
                eprintln!(
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
mod tests {
    use super::*;

    #[test]
    fn pick_best_result_prefers_longest_diff() {
        let winner = CandidateResult {
            task_id: TaskId::generate(),
            agent_label: "kilo".to_string(),
            status: TaskStatus::Done,
            diff_line_count: 12,
            metric_score: None,
        };
        let runner = CandidateResult {
            task_id: TaskId::generate(),
            agent_label: "cursor".to_string(),
            status: TaskStatus::Done,
            diff_line_count: 3,
            metric_score: None,
        };
        let failed = CandidateResult {
            task_id: TaskId::generate(),
            agent_label: "gemini".to_string(),
            status: TaskStatus::Failed,
            diff_line_count: 0,
            metric_score: None,
        };
        let results = vec![winner.clone(), runner, failed];
        let best = pick_best_result(&results).unwrap();
        assert_eq!(best.task_id, winner.task_id);
    }

    #[test]
    fn pick_best_result_none_when_no_done() {
        let failed = CandidateResult {
            task_id: TaskId::generate(),
            agent_label: "opencode".to_string(),
            status: TaskStatus::Failed,
            diff_line_count: 0,
            metric_score: None,
        };
        assert!(pick_best_result(&[failed]).is_none());
    }

    #[test]
    fn pick_best_result_prefers_metric_score() {
        let candidates = vec![
            CandidateResult {
                task_id: TaskId("t-1".into()),
                agent_label: "a".into(),
                status: TaskStatus::Done,
                diff_line_count: 100,
                metric_score: Some(3.0),
            },
            CandidateResult {
                task_id: TaskId("t-2".into()),
                agent_label: "b".into(),
                status: TaskStatus::Done,
                diff_line_count: 10,
                metric_score: Some(9.0),
            },
        ];
        let best = pick_best_result(&candidates).unwrap();
        assert_eq!(best.task_id, TaskId("t-2".into()));
    }

    #[test]
    fn best_of_count_validation() {
        assert!(validate_best_of_count(2).is_ok());
        assert!(validate_best_of_count(5).is_ok());
        assert!(validate_best_of_count(1).is_err());
        assert!(validate_best_of_count(0).is_err());
    }
}

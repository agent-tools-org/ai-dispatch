// Handler for `aid benchmark` — compare one prompt across multiple agents.
// Exports run(); depends on cmd::run, Store, task status, and cost formatting.

use anyhow::{Result, anyhow, ensure};
use std::sync::Arc;
use tokio::time::{Duration, sleep};

use crate::cmd::run::{self, RunArgs};
use crate::cost;
use crate::store::Store;
use crate::types::TaskId;

pub async fn run(store: Arc<Store>, prompt: String, agents: String, dir: Option<String>, verify: Option<String>) -> Result<()> {
    let agent_list = parse_agents(&agents)?;
    let mut task_ids = Vec::with_capacity(agent_list.len());

    for agent_name in &agent_list {
        let task_id = run::run(
            store.clone(),
            RunArgs {
                agent_name: agent_name.clone(),
                prompt: prompt.clone(),
                dir: dir.clone(),
                output: None,
                model: None,
                worktree: Some(format!("bench/{agent_name}")),
                base_branch: None,
                group: None,
                verify: verify.clone(),
                max_duration_mins: None,
                retry: 0,
                context: vec![],
                skills: vec![],
                background: true,
                announce: true,
                parent_task_id: None,
                on_done: None,
                fallback: None,
                template: None,
                repo: None,
                read_only: false,
            },
        )
        .await?;
        task_ids.push((agent_name.clone(), task_id));
    }

    println!("Waiting for {} agents...", task_ids.len());
    wait_for_completion(&store, &task_ids).await?;
    print_report(&store, &task_ids)
}

fn parse_agents(agents: &str) -> Result<Vec<String>> {
    let agent_list: Vec<String> = agents
        .split(',')
        .map(str::trim)
        .filter(|agent| !agent.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    ensure!(!agent_list.is_empty(), "No agents provided");
    Ok(agent_list)
}

async fn wait_for_completion(store: &Arc<Store>, task_ids: &[(String, TaskId)]) -> Result<()> {
    loop {
        let mut all_done = true;
        for (_, task_id) in task_ids {
            let Some(task) = store.get_task(task_id.as_str())? else {
                all_done = false;
                break;
            };
            if !task.status.is_terminal() {
                all_done = false;
                break;
            }
        }
        if all_done {
            return Ok(());
        }
        sleep(Duration::from_secs(3)).await;
    }
}

fn print_report(store: &Store, task_ids: &[(String, TaskId)]) -> Result<()> {
    println!("\n=== Benchmark Results ===");
    println!("{:<12} {:<8} {:<10} {:<10} {:<8}", "Agent", "Status", "Duration", "Tokens", "Cost");
    for (agent_name, task_id) in task_ids {
        let task = store.get_task(task_id.as_str())?.ok_or_else(|| anyhow!("Task '{}' not found", task_id.as_str()))?;
        let duration = task.duration_ms.map(format_duration).unwrap_or("-".to_string());
        let tokens = task.tokens.map(|tokens| tokens.to_string()).unwrap_or("-".to_string());
        println!(
            "{:<12} {:<8} {:<10} {:<10} {:<8}",
            agent_name,
            task.status.label(),
            duration,
            tokens,
            cost::format_cost(task.cost_usd)
        );
    }
    Ok(())
}

fn format_duration(duration_ms: i64) -> String { format!("{}s", duration_ms / 1000) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agents_trims_and_requires_values() {
        assert_eq!(parse_agents(" codex, opencode ").unwrap(), vec!["codex".to_string(), "opencode".to_string()]);
        assert!(parse_agents(" , ").is_err());
    }

    #[test]
    fn format_duration_uses_whole_seconds() {
        assert_eq!(format_duration(5_999), "5s");
    }
}

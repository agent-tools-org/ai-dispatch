// Batch dispatch concurrency helpers for auto caps and agent-set tracking.
// Exports: effective_max_active() for the dispatch loop.
// Deps: crate::batch, crate::store::Store, super::batch_dispatch_support

use crate::batch;
use crate::store::Store;
use std::collections::HashSet;

use super::super::batch_dispatch_support::pre_dispatch_fallback_choice;

pub(super) fn effective_max_active(
    store: &Store,
    tasks: &[batch::BatchTask],
    ready: &[usize],
    active: &[(usize, String)],
    max_concurrent: Option<usize>,
) -> anyhow::Result<usize> {
    if let Some(limit) = max_concurrent {
        return Ok(limit.max(1));
    }
    let default_max = crate::system_resources::recommended_max_concurrent()
        .min(tasks.len())
        .max(1);
    let agent_cap = unique_effective_agent_count(store, tasks, ready, active)?.max(1);
    Ok(default_max.min(agent_cap))
}

fn unique_effective_agent_count(
    store: &Store,
    tasks: &[batch::BatchTask],
    ready: &[usize],
    active: &[(usize, String)],
) -> anyhow::Result<usize> {
    let mut agents = HashSet::new();
    for (task_idx, task_id) in active {
        let current_agent = store
            .get_task(task_id)?
            .map(|task| task.agent.to_string())
            .unwrap_or_else(|| configured_agent_name(&tasks[*task_idx]));
        extend_effective_agents(
            &mut agents,
            current_agent.as_str(),
            tasks[*task_idx].fallback.as_deref(),
        );
    }
    for task_idx in ready {
        extend_effective_agents(
            &mut agents,
            configured_agent_name(&tasks[*task_idx]).as_str(),
            tasks[*task_idx].fallback.as_deref(),
        );
    }
    Ok(agents.len())
}

fn extend_effective_agents(agents: &mut HashSet<String>, current_agent: &str, fallback: Option<&str>) {
    if let Some((fallback_agent, remaining_cascade)) =
        pre_dispatch_fallback_choice(current_agent, fallback)
    {
        agents.insert(fallback_agent.as_str().to_string());
        agents.extend(remaining_cascade);
        return;
    }
    if !current_agent.trim().is_empty() {
        agents.insert(current_agent.to_string());
    }
    agents.extend(configured_fallback_agents_after(current_agent, fallback));
}

fn configured_agent_name(task: &batch::BatchTask) -> String {
    if task.agent.is_empty() {
        "auto".to_string()
    } else {
        task.agent.clone()
    }
}

fn configured_fallback_agents_after(current_agent: &str, fallback: Option<&str>) -> Vec<String> {
    let fallback_agents: Vec<_> = fallback
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|agent_name| !agent_name.is_empty())
        .map(ToString::to_string)
        .collect();
    let start = fallback_agents
        .iter()
        .position(|candidate| candidate == current_agent)
        .map(|idx| idx + 1)
        .unwrap_or(0);
    fallback_agents[start..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use crate::rate_limit::{clear_rate_limit, mark_rate_limited};
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;
    use tempfile::TempDir;

    fn make_task(agent: &str, fallback: Option<&str>) -> batch::BatchTask {
        batch::BatchTask {
            id: None,
            name: None,
            agent: agent.to_string(),
            team: None,
            prompt: "prompt".to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            judge: None,
            peer_review: None,
            best_of: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: fallback.map(str::to_string),
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            budget: false,
            env: None,
            env_forward: None,
            on_success: None,
            on_fail: None,
            conditional: false,
        }
    }

    fn stored_task(id: &str, agent: AgentKind) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Running,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        }
    }

    fn isolated_rate_limit_home() -> (TempDir, AidHomeGuard) {
        let temp_dir = TempDir::new().unwrap();
        let guard = AidHomeGuard::set(temp_dir.path());
        std::fs::create_dir_all(crate::paths::aid_dir()).unwrap();
        (temp_dir, guard)
    }

    #[test]
    fn explicit_max_concurrent_is_always_respected() {
        let store = Store::open_memory().unwrap();
        let tasks = [make_task("codex", Some("cursor"))];

        let limit = effective_max_active(&store, &tasks, &[0], &[], Some(7)).unwrap();

        assert_eq!(limit, 7);
    }

    #[test]
    fn auto_limit_counts_ready_fallback_targets_after_rate_limit() {
        let (_temp, _guard) = isolated_rate_limit_home();
        mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");
        let store = Store::open_memory().unwrap();
        let tasks = [
            make_task("codex", Some("opencode,cursor")),
            make_task("codex", Some("opencode,cursor")),
        ];

        let limit = effective_max_active(&store, &tasks, &[0], &[], None).unwrap();

        assert_eq!(limit, 2);
        clear_rate_limit(&AgentKind::Codex);
    }

    #[test]
    fn auto_limit_counts_active_agent_and_remaining_cascade() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&stored_task("t-1", AgentKind::OpenCode)).unwrap();
        let tasks = [
            make_task("codex", Some("opencode,cursor")),
            make_task("codex", Some("opencode,cursor")),
        ];

        let limit = effective_max_active(
            &store,
            &tasks,
            &[],
            &[(0, "t-1".to_string())],
            None,
        )
        .unwrap();

        assert_eq!(limit, 2);
    }
}

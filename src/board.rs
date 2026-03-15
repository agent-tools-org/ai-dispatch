// Text rendering for task board and task detail views.
// Board rows can enrich output with stored milestone events.

use anyhow::Result;

use crate::cost;
use crate::session;
use crate::store::Store;
use crate::types::*;

/// Render a summary table of tasks (for `aid board`)
pub fn render_board(tasks: &[Task], store: &Store) -> Result<String> {
    if tasks.is_empty() {
        return Ok("No tasks found.".to_string());
    }

    let (done, running, failed) = count_statuses(tasks);
    let total_tokens: i64 = tasks.iter().filter_map(|t| t.tokens).sum();
    let total_cost: f64 = tasks.iter().filter_map(|t| t.cost_usd).sum();

    let mut out = String::new();
    out.push_str(&format!(
        "Tasks: {} total | {} done | {} running | {} failed\n",
        tasks.len(), done, running, failed,
    ));
    if total_tokens > 0 {
        out.push_str(&format!("Total tokens: {}", format_tokens(total_tokens)));
        if total_cost > 0.0 {
            out.push_str(&format!("  Cost: {}", cost::format_cost(Some(total_cost))));
        }
        out.push('\n');
    }
    out.push('\n');

    let show_repo = tasks.iter().any(|task| task.repo_path.is_some());

    // Header
    if show_repo {
        out.push_str(&format!(
            "{:<10} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<10} {:<20} {:<16} {}\n",
            "ID", "Agent", "Status", "Duration", "Tokens", "Cost", "Parent", "Group", "Repo", "Caller", "Model"
        ));
        out.push_str(&"-".repeat(165));
        out.push('\n');
    } else {
        out.push_str(&format!(
            "{:<10} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<10} {:<16} {}\n",
            "ID", "Agent", "Status", "Duration", "Tokens", "Cost", "Parent", "Group", "Caller", "Model"
        ));
        out.push_str(&"-".repeat(144));
        out.push('\n');
    }

    for task in tasks {
        let status = if task.status == TaskStatus::AwaitingInput {
            let reason = store.get_events(task.id.as_str())
                .ok()
                .and_then(|evs| evs.into_iter().rev()
                    .find(|e| e.metadata.as_ref()
                        .and_then(|m| m.get("awaiting_input"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false))
                    .and_then(|e| e.metadata.as_ref()
                        .and_then(|m| m.get("awaiting_prompt"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())));
            match reason {
                Some(r) => truncate(&format!("AWAIT — {}", r), 30),
                None => task.status.label().to_string(),
            }
        } else {
            let base = task_status(task, store.latest_milestone(task.id.as_str())?);
            if task.verify_status == VerifyStatus::Failed {
                format!("{} [VFAIL]", base)
            } else {
                base
            }
        };
        let duration = if task.status == TaskStatus::Skipped {
            "-".to_string()
        } else {
            task.duration_ms
                .map(format_duration)
                .unwrap_or_else(|| elapsed_since(task.created_at))
        };
        let tokens = if task.status == TaskStatus::Skipped {
            "-".to_string()
        } else {
            task.tokens
                .map(format_tokens)
                .unwrap_or_else(|| "-".to_string())
        };
        let cost_str = if task.status == TaskStatus::Skipped {
            "-".to_string()
        } else {
            cost::format_cost_label(task.cost_usd, task.agent)
        };
        let parent = short_parent(task.parent_task_id.as_deref());
        let group = short_group(task.workgroup_id.as_deref());
        let repo = short_repo(task.repo_path.as_deref());
        let caller = session::display(task);
        let model = task.model
            .as_deref()
            .unwrap_or("-");

        if show_repo {
            out.push_str(&format!(
                "{:<10} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<10} {:<20} {:<16} {}\n",
                task.id.as_str(),
                task.agent_display_name(),
                status,
                duration,
                tokens,
                cost_str,
                parent,
                group,
                repo,
                caller,
                model,
            ));
        } else {
            out.push_str(&format!(
                "{:<10} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<10} {:<16} {}\n",
                task.id.as_str(),
                task.agent_display_name(),
                status,
                duration,
                tokens,
                cost_str,
                parent,
                group,
                caller,
                model,
            ));
        }
    }
    Ok(out)
}

/// Render detailed view of a single task (for `aid audit`)
pub fn render_task_detail(task: &Task, events: &[TaskEvent], retry_chain: Option<Vec<Task>>) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Task: {} — {}: {}\n",
        task.id,
        task.agent_display_name(),
        truncate(&task.prompt, 60),
    ));

    let duration = task.duration_ms
        .map(format_duration)
        .unwrap_or_else(|| elapsed_since(task.created_at));
    out.push_str(&format!("Status: {}  Duration: {}\n", task.status.label(), duration));
    if let Some(parent) = task.parent_task_id.as_deref() {
        out.push_str(&format!("Parent: {parent}\n"));
        if let Some(retry_chain) = retry_chain.as_deref()
            && retry_chain.len() > 1
        {
            out.push_str("Retry chain:\n");
            for retry_task in retry_chain {
                let duration = retry_task.duration_ms
                    .map(format_duration)
                    .unwrap_or_else(|| elapsed_since(retry_task.created_at));
                let current = if retry_task.id == task.id {
                    "  ← current"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "  {} ({})  → {:<7} {:>5}  {}{}\n",
                    retry_task.id,
                    retry_kind(retry_task),
                    retry_status(retry_task.status),
                    duration,
                    cost::format_cost(retry_task.cost_usd),
                    current,
                ));
            }
        }
    }
    if let Some(group_id) = task.workgroup_id.as_deref() {
        out.push_str(&format!("Workgroup: {group_id}\n"));
    }
    if let Some(repo_path) = task.repo_path.as_deref() {
        out.push_str(&format!("Repo: {repo_path}\n"));
    }
    if task.caller_kind.is_some() || task.caller_session_id.is_some() {
        out.push_str(&format!("Caller: {}\n", session::display(task)));
    }

    if let Some(tokens) = task.tokens {
        out.push_str(&format!("Tokens: {}", format_tokens(tokens)));
        if let Some(c) = task.cost_usd {
            out.push_str(&format!("  Cost: {}", cost::format_cost(Some(c))));
        }
        out.push('\n');
    }
    if let Some(prompt_tokens) = task.prompt_tokens {
        let bytes = task.resolved_prompt.as_deref().map(|p| p.len()).unwrap_or(0);
        out.push_str(&format!("Prompt: ~{} tokens ({} bytes)\n", prompt_tokens, bytes));
    }
    if let Some(ref model) = task.model {
        out.push_str(&format!("Model: {}\n", model));
    }
    if let Some(ref wt) = task.worktree_path {
        out.push_str(&format!("Worktree: {}", wt));
        if let Some(ref branch) = task.worktree_branch {
            out.push_str(&format!(" ({})", branch));
        }
        out.push('\n');
    }
    if let Some(ref log) = task.log_path {
        out.push_str(&format!("Log: {}\n", log));
    }
    if let Some(ref output) = task.output_path {
        out.push_str(&format!("Output: {}\n", output));
    }

    if !events.is_empty() {
        out.push_str("\nEvents:\n");
        for ev in events {
            let time = ev.timestamp.format("%H:%M:%S");
            out.push_str(&format!(
                "  {}  [{:>10}] {}\n",
                time,
                ev.event_kind.as_str(),
                truncate(&ev.detail, 60),
            ));
        }
    }
    out
}

fn count_statuses(tasks: &[Task]) -> (usize, usize, usize) {
    let mut done = 0;
    let mut running = 0;
    let mut failed = 0;
    for t in tasks {
        match t.status {
            TaskStatus::Done | TaskStatus::Merged => done += 1,
            TaskStatus::Running | TaskStatus::AwaitingInput => running += 1,
            TaskStatus::Failed => failed += 1,
            TaskStatus::Pending | TaskStatus::Skipped => {}
        }
    }
    (done, running, failed)
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn elapsed_since(start: chrono::DateTime<chrono::Local>) -> String {
    let elapsed = chrono::Local::now() - start;
    let secs = elapsed.num_seconds();
    if secs < 0 {
        "0s".to_string()
    } else if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

fn short_parent(parent: Option<&str>) -> String {
    parent.unwrap_or("-").to_string()
}

fn short_group(group: Option<&str>) -> String {
    group.unwrap_or("-").to_string()
}

fn short_repo(repo: Option<&str>) -> String {
    repo.map(|path| truncate(path, 20))
        .unwrap_or_else(|| "-".to_string())
}

fn task_status(task: &Task, milestone: Option<String>) -> String {
    if task.status == TaskStatus::Running
        && let Some(milestone) = milestone
    {
        return truncate(&format!("{} — {}", task.status.label(), milestone), 30);
    }
    task.status.label().to_string()
}

fn retry_kind(task: &Task) -> &'static str {
    if task.parent_task_id.is_some() {
        "retry"
    } else {
        "root"
    }
}

fn retry_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "Pending",
        TaskStatus::Running => "Running",
        TaskStatus::AwaitingInput => "Await",
        TaskStatus::Done => "Done",
        TaskStatus::Merged => "Merged",
        TaskStatus::Failed => "Failed",
        TaskStatus::Skipped => "Skipped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::store::Store;
    use serde_json::json;

    fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
            custom_agent_name: None,
            prompt: "test prompt".to_string(),
            resolved_prompt: None,
            status,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: Some("feat/test".to_string()),
            log_path: None,
            output_path: None,
            tokens: Some(45000),
            prompt_tokens: None,
            duration_ms: Some(227000),
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn empty_board() {
        let store = Store::open_memory().unwrap();
        assert_eq!(render_board(&[], &store).unwrap(), "No tasks found.");
    }

    #[test]
    fn board_with_tasks() {
        let store = Store::open_memory().unwrap();
        let tasks = vec![
            make_task("t-0001", AgentKind::Codex, TaskStatus::Done),
            make_task("t-0002", AgentKind::Gemini, TaskStatus::Running),
        ];
        let output = render_board(&tasks, &store).unwrap();
        assert!(output.contains("t-0001"));
        assert!(output.contains("codex"));
        assert!(output.contains("DONE"));
        assert!(output.contains("RUN"));
        assert!(output.contains("2 total"));
        assert!(output.contains("Cost"));
        assert!(output.contains("Caller"));
        assert!(output.contains("Group"));
    }

    #[test]
    fn board_shows_running_task_milestone() {
        let store = Store::open_memory().unwrap();
        let task = make_task("t-0003", AgentKind::Codex, TaskStatus::Running);
        store.insert_task(&task).unwrap();
        store.insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "types defined".to_string(),
            metadata: None,
        }).unwrap();

        let output = render_board(&[task], &store).unwrap();
        assert!(output.contains("RUN — types defined"));
    }

    #[test]
    fn board_shows_awaiting_input_reason() {
        let store = Store::open_memory().unwrap();
        let task = make_task("t-0004", AgentKind::Codex, TaskStatus::AwaitingInput);
        store.insert_task(&task).unwrap();
        store.insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "115:    use super::board::render_board;".to_string(),
            metadata: Some(json!({ "awaiting_input": true, "awaiting_prompt": "Continue with fix?" })),
        }).unwrap();

        let output = render_board(&[task], &store).unwrap();
        assert!(output.contains("AWAIT — Continue with fix?"));
        assert!(!output.contains("115:    use super::board::render_board;"));
    }

    #[test]
    fn board_shows_repo_column_when_present() {
        let store = Store::open_memory().unwrap();
        let mut task = make_task("t-0005", AgentKind::Codex, TaskStatus::Done);
        task.repo_path = Some("/tmp/example-repo".to_string());

        let output = render_board(&[task], &store).unwrap();
        assert!(output.contains("Repo"));
        assert!(output.contains("/tmp/example-repo"));
    }

    #[test]
    fn task_detail_rendering() {
        let task = make_task("t-0001", AgentKind::Codex, TaskStatus::Done);
        let events = vec![TaskEvent {
            task_id: TaskId("t-0001".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::ToolCall,
            detail: "exec: cargo test".to_string(),
            metadata: None,
        }];
        let output = render_task_detail(&task, &events, None);
        assert!(output.contains("t-0001"));
        assert!(output.contains("cargo test"));
    }

    #[test]
    fn task_detail_shows_retry_chain() {
        let mut root = make_task("t-1001", AgentKind::Codex, TaskStatus::Done);
        root.duration_ms = Some(12_000);
        root.cost_usd = Some(0.03);
        let mut retry_1 = make_task("t-1002", AgentKind::Codex, TaskStatus::Failed);
        retry_1.parent_task_id = Some("t-1001".to_string());
        retry_1.duration_ms = Some(8_000);
        retry_1.cost_usd = Some(0.02);
        let mut retry_2 = make_task("t-1003", AgentKind::Codex, TaskStatus::Done);
        retry_2.parent_task_id = Some("t-1002".to_string());
        retry_2.duration_ms = Some(15_000);
        retry_2.cost_usd = Some(0.04);

        let output = render_task_detail(&retry_2, &[], Some(vec![root, retry_1, retry_2.clone()]));
        assert!(output.contains("Retry chain:"));
        assert!(output.contains("t-1001 (root)  → Done"));
        assert!(output.contains("t-1002 (retry)  → Failed"));
        assert!(output.contains("t-1003 (retry)  → Done"));
        assert!(output.contains("← current"));
    }
}

// Text rendering for task board and task detail views.
// Pure functions — no I/O, easy to test.

use crate::cost;
use crate::types::*;

/// Render a summary table of tasks (for `aid board`)
pub fn render_board(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        return "No tasks found.".to_string();
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

    // Header
    out.push_str(&format!(
        "{:<10} {:<10} {:<6} {:<10} {:<10} {:<8} {}\n",
        "ID", "Agent", "Status", "Duration", "Tokens", "Cost", "Model"
    ));
    out.push_str(&"-".repeat(78));
    out.push('\n');

    for task in tasks {
        let duration = task.duration_ms
            .map(format_duration)
            .unwrap_or_else(|| elapsed_since(task.created_at));
        let tokens = task.tokens
            .map(format_tokens)
            .unwrap_or_else(|| "-".to_string());
        let cost_str = cost::format_cost(task.cost_usd);
        let model = task.model
            .as_deref()
            .unwrap_or("-");

        out.push_str(&format!(
            "{:<10} {:<10} {:<6} {:<10} {:<10} {:<8} {}\n",
            task.id.as_str(),
            task.agent.as_str(),
            task.status.label(),
            duration,
            tokens,
            cost_str,
            model,
        ));
    }
    out
}

/// Render detailed view of a single task (for `aid audit`)
pub fn render_task_detail(task: &Task, events: &[TaskEvent]) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Task: {} — {}: {}\n",
        task.id, task.agent, truncate(&task.prompt, 60)
    ));

    let duration = task.duration_ms
        .map(format_duration)
        .unwrap_or_else(|| elapsed_since(task.created_at));
    out.push_str(&format!("Status: {}  Duration: {}\n", task.status.label(), duration));

    if let Some(tokens) = task.tokens {
        out.push_str(&format!("Tokens: {}", format_tokens(tokens)));
        if let Some(c) = task.cost_usd {
            out.push_str(&format!("  Cost: {}", cost::format_cost(Some(c))));
        }
        out.push('\n');
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
            TaskStatus::Done => done += 1,
            TaskStatus::Running => running += 1,
            TaskStatus::Failed => failed += 1,
            TaskStatus::Pending => {}
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
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
            prompt: "test prompt".to_string(),
            status,
            parent_task_id: None,
            worktree_path: None,
            worktree_branch: Some("feat/test".to_string()),
            log_path: None,
            output_path: None,
            tokens: Some(45000),
            duration_ms: Some(227000),
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
        }
    }

    #[test]
    fn empty_board() {
        assert_eq!(render_board(&[]), "No tasks found.");
    }

    #[test]
    fn board_with_tasks() {
        let tasks = vec![
            make_task("t-0001", AgentKind::Codex, TaskStatus::Done),
            make_task("t-0002", AgentKind::Gemini, TaskStatus::Running),
        ];
        let output = render_board(&tasks);
        assert!(output.contains("t-0001"));
        assert!(output.contains("codex"));
        assert!(output.contains("DONE"));
        assert!(output.contains("RUN"));
        assert!(output.contains("2 total"));
        assert!(output.contains("Cost"));
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
        let output = render_task_detail(&task, &events);
        assert!(output.contains("t-0001"));
        assert!(output.contains("cargo test"));
    }
}

// Handler for `aid show <task-id>` — unified task inspection.
// Combines events, diff, output, log, and AI explanation into one command.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::cmd;
use crate::paths;
use crate::store::Store;
use crate::types::{Task, TaskStatus};

#[path = "show_output.rs"]
mod show_output;

pub use show_output::{diff_text, log_text, output_text, output_text_for_task};
#[allow(unused_imports)]
pub use show_output::read_task_output;
pub(crate) use show_output::{diff_stat, read_tail};

pub struct ShowArgs {
    pub task_id: String,
    pub context: bool,
    pub diff: bool,
    pub output: bool,
    pub explain: bool,
    pub log: bool,
    pub agent: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Copy)]
pub enum ShowMode {
    Summary,
    Context,
    Diff,
    Output,
    Log,
}

pub async fn run(store: Arc<Store>, args: ShowArgs) -> Result<()> {
    if args.context {
        let text = render_mode_text(&store, &args.task_id, ShowMode::Context)?;
        print!("{text}");
        return Ok(());
    }
    if args.explain {
        return cmd::explain::run_explain(store, &args.task_id, args.agent, args.model).await;
    }
    let mode = if args.diff {
        ShowMode::Diff
    } else if args.output {
        ShowMode::Output
    } else if args.log {
        ShowMode::Log
    } else {
        ShowMode::Summary
    };
    let text = render_mode_text(&store, &args.task_id, mode)?;
    print!("{text}");
    Ok(())
}

pub fn render_mode_text(store: &Arc<Store>, task_id: &str, mode: ShowMode) -> Result<String> {
    match mode {
        ShowMode::Summary => audit_text(store, task_id),
        ShowMode::Context => context_text(store, task_id),
        ShowMode::Diff => diff_text(store, task_id),
        ShowMode::Output => output_text(store, task_id),
        ShowMode::Log => log_text(task_id),
    }
}

// --- Default mode: events + stderr + diff stat ---

pub fn audit_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let retry_chain = if task.parent_task_id.is_some() {
        Some(store.get_retry_chain(task_id)?)
    } else {
        None
    };
    let mut out = render_task_detail(&task, &events, retry_chain);

    if task.status == TaskStatus::Failed
        && let Some(stderr) = stderr_tail(task_id)
    {
        out.push_str("\nStderr:\n");
        out.push_str(&stderr);
    }

    if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\nChanges:\n");
        out.push_str(&diff_stat(wt_path));
    }

    Ok(out)
}

pub fn context_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str("=== Original Prompt ===\n");
    out.push_str(&task.prompt);
    out.push_str("\n");

    if let Some(resolved_prompt) = task.resolved_prompt.as_deref() {
        out.push_str("\n=== Resolved Prompt ===\n");
        out.push_str(resolved_prompt);
        out.push_str("\n");
        return Ok(out);
    }

    let (skill_content, resolved_prompt) = reconstruct_context(store, &task)?;
    out.push_str("\n(reconstructed — context files may have changed since dispatch)\n");
    out.push_str("\n=== Injected Skills ===\n");
    if skill_content.trim().is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&skill_content);
        out.push_str("\n");
    }
    out.push_str("\n=== Resolved Prompt ===\n");
    out.push_str(&resolved_prompt);
    out.push_str("\n");
    Ok(out)
}

// --- Shared helpers ---

pub(crate) fn load_task(store: &Arc<Store>, task_id: &str) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}

fn reconstruct_context(store: &Arc<Store>, task: &Task) -> Result<(String, String)> {
    let workgroup = if let Some(group_id) = task.workgroup_id.as_deref() {
        store.get_workgroup(group_id)?
    } else {
        None
    };
    let milestones = if let Some(group_id) = task.workgroup_id.as_deref() {
        store.get_workgroup_milestones(group_id)?
    } else {
        vec![]
    };
    let skill_names = crate::skills::auto_skills(&task.agent, task.worktree_path.is_some());
    let skill_parts = skill_names
        .iter()
        .map(|skill| {
            crate::skills::resolve_skill_content(skill)
                .unwrap_or_else(|err| format!("[missing skill: {skill}: {err}]"))
        })
        .collect::<Vec<_>>();
    let skill_content = skill_parts.join("\n\n");
    let mut resolved_prompt = crate::workgroup::compose_prompt(
        &task.prompt,
        None,
        workgroup.as_ref(),
        &milestones,
    );
    if !skill_content.is_empty() {
        resolved_prompt = format!("{resolved_prompt}\n\n--- Methodology ---\n{skill_content}");
    }
    resolved_prompt = crate::templates::inject_milestone_prompt(&resolved_prompt);
    Ok((skill_content, resolved_prompt))
}

fn stderr_tail(task_id: &str) -> Option<String> {
    let content = std::fs::read_to_string(paths::stderr_path(task_id)).ok()?;
    if content.is_empty() {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(20);
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("  ... ({start} lines omitted)\n"));
    }
    for line in &lines[start..] {
        out.push_str(&format!("  {line}\n"));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId};
    use chrono::Local;
    use std::sync::Arc;

    #[test]
    fn context_text_prefers_stored_resolved_prompt() {
        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-context".to_string()),
            agent: AgentKind::Codex,
            prompt: "raw prompt".to_string(),
            resolved_prompt: Some("resolved prompt".to_string()),
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
        };
        store.insert_task(&task).unwrap();

        let text = context_text(&store, "t-context").unwrap();

        assert!(text.contains("=== Original Prompt ===\nraw prompt"));
        assert!(text.contains("=== Resolved Prompt ===\nresolved prompt"));
        assert!(!text.contains("(reconstructed"));
    }

    #[test]
    fn context_text_reconstructs_skills_when_resolved_prompt_missing() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = crate::paths::aid_dir().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-reconstruct".to_string()),
            agent: AgentKind::Codex,
            prompt: "raw prompt".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
        };
        store.insert_task(&task).unwrap();

        let text = context_text(&store, "t-reconstruct").unwrap();

        assert!(text.contains("(reconstructed"));
        assert!(text.contains("=== Injected Skills ===\n# Implementer"));
        assert!(text.contains("=== Resolved Prompt ===\nraw prompt"));
        assert!(text.contains("[MILESTONE] <brief description>"));
    }
}

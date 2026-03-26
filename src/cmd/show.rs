// Handler for `aid show <task-id>` — unified task inspection.
// Combines events, diff, output, log, and AI explanation into one command.

use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::cmd;
use crate::paths;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus, VerifyStatus};

#[path = "show_output.rs"]
mod show_output;

pub use show_output::{
    diff_text, log_text, output_text, output_text_brief, output_text_for_task,
};
#[allow(unused_imports)]
pub use show_output::output_text_full;
#[allow(unused_imports)]
pub use show_output::read_task_output;
pub(crate) use show_output::{
    diff_stat, diff_text_file, extract_messages_from_log, parse_diff_stat, read_tail,
    worktree_diff,
};

pub struct ShowArgs {
    pub task_id: String,
    pub context: bool,
    pub diff: bool,
    pub summary: bool,
    pub file: Option<String>,
    pub output: bool,
    pub full: bool,
    pub brief: bool,
    pub explain: bool,
    pub log: bool,
    pub json: bool,
    pub agent: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Copy)]
pub enum ShowMode {
    Summary,
    StatOnly,
    Context,
    Diff,
    Output,
    Log,
}

pub async fn run(store: Arc<Store>, args: ShowArgs) -> Result<()> {
    if args.json {
        let text = task_json(&store, &args.task_id)?;
        println!("{text}");
        return Ok(());
    }
    if args.context {
        let text = render_mode_text(&store, &args.task_id, ShowMode::Context)?;
        print!("{text}");
        return Ok(());
    }
    if args.explain {
        return cmd::explain::run_explain(store, &args.task_id, args.agent, args.model).await;
    }
    let mode = if args.summary {
        ShowMode::StatOnly
    } else if args.diff {
        ShowMode::Diff
    } else if args.output {
        ShowMode::Output
    } else if args.log {
        ShowMode::Log
    } else {
        ShowMode::Summary
    };
    let _ = args.full;
    let task = load_task(&store, &args.task_id)?;
    let text = if matches!(mode, ShowMode::Output) && args.brief {
        render_output_brief_text(&store, &args.task_id)?
    } else if matches!(mode, ShowMode::Diff) {
        if let Some(file) = args.file.as_deref() {
            diff_text_file(&store, &args.task_id, file)?
        } else {
            diff_text(&store, &args.task_id)?
        }
    } else {
        render_mode_text(&store, &args.task_id, mode)?
    };
    print!("{text}");
    if matches!(mode, ShowMode::Diff) {
        aid_hint!(
            "[aid] Actions: aid merge {} | aid retry {} -f \"feedback\"",
            args.task_id, args.task_id
        );
    }
    if !task.status.is_terminal() {
        aid_hint!(
            "[aid] Task is still running. To wait for completion: aid watch --quiet {}",
            args.task_id
        );
    }
    Ok(())
}

/// Serialize task as JSON with events and metrics.
fn task_json(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let event_list: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "timestamp": e.timestamp.to_rfc3339(),
                "type": e.event_kind.as_str(),
                "detail": e.detail,
                "metadata": e.metadata,
            })
        })
        .collect();
    let diff_entries = task
        .worktree_path
        .as_deref()
        .filter(|path| Path::new(path).exists())
        .map(|path| parse_diff_stat(&diff_stat(path)))
        .unwrap_or_default();
    let files_changed = diff_entries.len();
    let (insertions, deletions) =
        diff_entries
            .iter()
            .fold((0u64, 0u64), |(ins, del), entry| {
                (
                    ins + entry["insertions"].as_u64().unwrap_or(0),
                    del + entry["deletions"].as_u64().unwrap_or(0),
                )
            });
    let output = output_text_for_task(store.as_ref(), task_id, true).ok();
    let payload = serde_json::json!({
        "id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "custom_agent": task.custom_agent_name,
        "status": task.status.as_str(),
        "prompt": task.prompt,
        "model": task.model,
        "tokens": task.tokens,
        "prompt_tokens": task.prompt_tokens,
        "duration_ms": task.duration_ms,
        "cost_usd": task.cost_usd,
        "workgroup_id": task.workgroup_id,
        "parent_task_id": task.parent_task_id,
        "worktree_branch": task.worktree_branch,
        "worktree_path": task.worktree_path,
        "repo_path": task.repo_path,
        "output_path": task.output_path,
        "output": output,
        "verify": task.verify,
        "exit_code": task.exit_code,
        "verify_status": task.verify_status.as_str(),
        "pending_reason": task.pending_reason,
        "read_only": task.read_only,
        "budget": task.budget,
        "created_at": task.created_at.to_rfc3339(),
        "completed_at": task.completed_at.map(|dt| dt.to_rfc3339()),
        "events": event_list,
        "diff_stat": diff_entries,
        "diff_summary": {
            "files_changed": files_changed,
            "insertions": insertions,
            "deletions": deletions,
        },
    });
    serde_json::to_string(&payload).map_err(Into::into)
}

fn render_output_brief_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let mut text = output_text_brief(store, task_id)?;
    let truncated = output_text(store, task_id)
        .map(|full_text| full_text != text)
        .unwrap_or(false);
    if truncated {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "[truncated — use `aid show {task_id} --output` for full content]"
        ));
    }
    Ok(text)
}

pub fn render_mode_text(store: &Arc<Store>, task_id: &str, mode: ShowMode) -> Result<String> {
    match mode {
        ShowMode::Summary => audit_text(store, task_id),
        ShowMode::StatOnly => summary_text(store, task_id),
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

    if task.verify_status == VerifyStatus::EmptyDiff {
        out.push_str("\nChanges:\n[no changes]\n");
    } else if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\nChanges:\n");
        out.push_str(&diff_stat(wt_path));
    } else if task.worktree_branch.is_none()
        && matches!(task.status, TaskStatus::Done | TaskStatus::Merged)
    {
        // In-place task: show working tree diff stat from repo
        let repo = task.repo_path.as_deref().unwrap_or(".");
        if let Some(stat) = inplace_diff_stat(repo) {
            out.push_str("\nWorking tree changes (in-place edit):\n");
            out.push_str(&stat);
        }
    }

    if !task_has_changes(&task) && task.status.is_terminal()
        && let Some(findings) = research_findings(store.as_ref(), &task)
    {
        out.push_str("\nFindings:\n");
        out.push_str(&findings);
        out.push('\n');
        aid_hint!(
            "[aid] Research task. Full output: aid show {} --output",
            task.id
        );
    }

    Ok(out)
}

pub fn summary_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Agent: {}  Status: {}  Prompt: {}\n",
        task.agent_display_name(),
        task.status.label(),
        task.prompt,
    ));

    if task.verify_status == VerifyStatus::EmptyDiff {
        out.push_str("\n--- Diff Stat ---\n  (no changes detected)\n");
    } else if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\n--- Diff Stat ---\n");
        out.push_str(&diff_stat(wt_path));
    } else if task.worktree_branch.is_none()
        && matches!(task.status, TaskStatus::Done | TaskStatus::Merged)
    {
        let repo = task.repo_path.as_deref().unwrap_or(".");
        out.push_str("\n--- Diff Stat ---\n");
        if let Some(stat) = inplace_diff_stat(repo) {
            out.push_str(&stat);
        } else {
            out.push_str("  (no changes detected)\n");
        }
    }

    if !out.contains("--- Diff Stat ---") || out.contains("(no changes detected)") {
        if let Some(conclusion) = completion_conclusion(store.as_ref(), task.id.as_str()) {
            out.push_str("\nConclusion: ");
            out.push_str(&conclusion);
            out.push('\n');
        }
    }

    Ok(out)
}

pub fn context_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str("=== Original Prompt ===\n");
    out.push_str(&task.prompt);
    out.push('\n');

    if let Some(resolved_prompt) = task.resolved_prompt.as_deref() {
        out.push_str("\n=== Resolved Prompt ===\n");
        out.push_str(resolved_prompt);
        out.push('\n');
        return Ok(out);
    }

    let (skill_content, resolved_prompt) = reconstruct_context(store, &task)?;
    out.push_str("\n(reconstructed — context files may have changed since dispatch)\n");
    out.push_str("\n=== Injected Skills ===\n");
    if skill_content.trim().is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&skill_content);
        out.push('\n');
    }
    out.push_str("\n=== Resolved Prompt ===\n");
    out.push_str(&resolved_prompt);
    out.push('\n');
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
        &[],
    );
    if !skill_content.is_empty() {
        resolved_prompt = format!("{resolved_prompt}\n\n--- Methodology ---\n{skill_content}");
    }
    resolved_prompt = crate::templates::inject_milestone_prompt(&resolved_prompt);
    Ok((skill_content, resolved_prompt))
}

fn inplace_diff_stat(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_path, "diff", "--stat"])
        .output()
        .ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn task_has_changes(task: &Task) -> bool {
    if task.verify_status == VerifyStatus::EmptyDiff {
        return false;
    }
    task.worktree_path
        .as_ref()
        .is_some_and(|path| Path::new(path).exists())
        || (task.worktree_branch.is_none()
            && inplace_diff_stat(task.repo_path.as_deref().unwrap_or(".")).is_some())
}

fn completion_conclusion(store: &Store, task_id: &str) -> Option<String> {
    let summary_json = store.get_completion_summary(task_id).ok()??;
    let summary =
        serde_json::from_str::<crate::cmd::summary::CompletionSummary>(&summary_json).ok()?;
    if summary.conclusion.is_empty() {
        None
    } else {
        Some(summary.conclusion)
    }
}

fn research_findings(store: &Store, task: &Task) -> Option<String> {
    if let Some(conclusion) = completion_conclusion(store, task.id.as_str()) {
        return Some(conclusion);
    }
    let log_path = task
        .log_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::log_path(task.id.as_str()));
    extract_messages_from_log(&log_path, false).filter(|messages| !messages.is_empty())
}

pub(crate) fn task_hook_json(
    task_id: &TaskId,
    agent: &str,
    status: TaskStatus,
    prompt: &str,
    worktree: Option<&str>,
    dir: Option<&str>,
    exit_code: Option<i32>,
) -> serde_json::Value {
    json!({
        "task_id": task_id.as_str(),
        "agent": agent,
        "status": status.as_str(),
        "prompt": prompt,
        "worktree": worktree,
        "dir": dir,
        "exit_code": exit_code,
    })
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
    use crate::cmd::summary::CompletionSummary;
    use crate::types::{AgentKind, TaskId, VerifyStatus};
    use chrono::Local;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    #[test]
    fn context_text_prefers_stored_resolved_prompt() {
        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-context".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "raw prompt".to_string(),
            resolved_prompt: Some("resolved prompt".to_string()),
            category: None,
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
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
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
            custom_agent_name: None,
            prompt: "raw prompt".to_string(),
            resolved_prompt: None,
            category: None,
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
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        };
        store.insert_task(&task).unwrap();

        let text = context_text(&store, "t-reconstruct").unwrap();

        assert!(text.contains("(reconstructed"));
        assert!(text.contains("=== Injected Skills ===\n# Implementer"));
        assert!(text.contains("=== Resolved Prompt ===\nraw prompt"));
        assert!(text.contains("[MILESTONE] <brief description>"));
    }

    #[test]
    fn summary_text_shows_diff_stat_without_full_diff() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("note.txt"), "before\n").unwrap();
        Command::new("git")
            .args(["add", "note.txt"])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("note.txt"), "before\nafter\n").unwrap();

        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-summary".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "summarize diff".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: Some(repo.display().to_string()),
            worktree_path: None,
            worktree_branch: None,
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
        };
        store.insert_task(&task).unwrap();

        let text = summary_text(&store, "t-summary").unwrap();

        assert!(text.contains("--- Diff Stat ---"));
        assert!(text.contains("note.txt | 1 +"));
        assert!(!text.contains("--- Full Diff ---"));
        assert!(!text.contains("@@"));
    }

    #[test]
    fn audit_text_shows_findings_for_research_task() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let store = Arc::new(Store::open_memory().unwrap());
        let task = research_task("t-audit-findings", temp.path());
        store.insert_task(&task).unwrap();
        save_completion_summary(&store, task.id.as_str(), "Investigated the failure mode.");

        let text = audit_text(&store, task.id.as_str()).unwrap();

        assert!(text.contains("Findings:\nInvestigated the failure mode."));
    }

    #[test]
    fn summary_text_shows_conclusion_for_research_task() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let store = Arc::new(Store::open_memory().unwrap());
        let task = research_task("t-summary-conclusion", temp.path());
        store.insert_task(&task).unwrap();
        save_completion_summary(&store, task.id.as_str(), "Summarized the research outcome.");

        let text = summary_text(&store, task.id.as_str()).unwrap();

        assert!(text.contains("--- Diff Stat ---"));
        assert!(text.contains("(no changes detected)"));
        assert!(text.contains("Conclusion: Summarized the research outcome."));
    }

    #[test]
    fn audit_text_shows_pending_reason() {
        let store = Arc::new(Store::open_memory().unwrap());
        let mut task = research_task("t-show-pending-reason", Path::new("."));
        task.status = TaskStatus::Failed;
        task.pending_reason = Some("agent_starting".to_string());
        store.insert_task(&task).unwrap();

        let text = audit_text(&store, task.id.as_str()).unwrap();

        assert!(text.contains("Pending reason: agent_starting"));
    }

    #[test]
    fn task_json_includes_pending_reason() {
        let store = Arc::new(Store::open_memory().unwrap());
        let mut task = research_task("t-show-json", Path::new("."));
        let output_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(output_file.path(), "full output\n").unwrap();
        task.pending_reason = Some("unknown".to_string());
        task.output_path = Some(output_file.path().display().to_string());
        store.insert_task(&task).unwrap();

        let payload: serde_json::Value = serde_json::from_str(&task_json(&store, task.id.as_str()).unwrap()).unwrap();

        assert_eq!(payload["pending_reason"], "unknown");
        assert_eq!(payload["output"], "full output\n");
    }

    fn init_git_repo(repo: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("note.txt"), "baseline\n").unwrap();
        Command::new("git")
            .args(["add", "note.txt"])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();
    }

    fn research_task(task_id: &str, repo: &Path) -> Task {
        Task {
            id: TaskId(task_id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "research task".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: Some(repo.display().to_string()),
            worktree_path: None,
            worktree_branch: None,
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

    fn save_completion_summary(store: &Store, task_id: &str, conclusion: &str) {
        let summary = CompletionSummary {
            task_id: task_id.to_string(),
            agent: "codex".to_string(),
            status: "done".to_string(),
            files_changed: vec![],
            summary_text: "research task completed".to_string(),
            conclusion: conclusion.to_string(),
            duration_secs: None,
            token_count: None,
        };
        store
            .save_completion_summary(task_id, &serde_json::to_string(&summary).unwrap())
            .unwrap();
    }
}

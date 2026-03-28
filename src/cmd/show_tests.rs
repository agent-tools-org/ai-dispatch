// Unit tests for `cmd::show` — context, summary/audit, JSON. Loaded via `#[path]` from `show.rs`.
// Deps: `super::*`, `chrono`, `tempfile`.

use super::show_json::task_json;
use super::*;
use crate::cmd::summary::CompletionSummary;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn task_fixture(
    id: &str,
    prompt: &str,
    resolved_prompt: Option<&str>,
    repo_path: Option<String>,
) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: prompt.to_string(),
        resolved_prompt: resolved_prompt.map(String::from),
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path,
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

fn research_task(task_id: &str, repo: &Path) -> Task {
    task_fixture(
        task_id,
        "research task",
        None,
        Some(repo.display().to_string()),
    )
}

#[test]
fn context_text_prefers_stored_resolved_prompt() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-context", "raw prompt", Some("resolved prompt"), None);
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
    let task = task_fixture("t-reconstruct", "raw prompt", None, None);
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
    let task = task_fixture(
        "t-summary",
        "summarize diff",
        None,
        Some(repo.display().to_string()),
    );
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

    let payload: serde_json::Value =
        serde_json::from_str(&task_json(&store, task.id.as_str()).unwrap()).unwrap();

    assert_eq!(payload["pending_reason"], "unknown");
    assert_eq!(payload["output"], "full output\n");
}

#[test]
fn result_text_reads_task_result_file() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let path = crate::paths::task_dir("t-result").join("result.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "structured result\n").unwrap();

    let text = result_text("t-result").unwrap();

    assert_eq!(text, "structured result\n");
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

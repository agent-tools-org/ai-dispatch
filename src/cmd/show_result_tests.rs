// Tests for persisted result-file output fallback in show helpers.
// Ensures `result.md` is treated as the primary rendered output when present.

use std::path::{Path, PathBuf};

use crate::cmd::show::read_task_output;
use crate::paths::AidHomeGuard;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

// Local RAII CWD guard (same pattern as src/state_tests.rs). Not hoisted:
// state_tests already has its own copy plus a module-local lock.
// CWD is process-global, so these tests can race other TempCwd users under
// the default parallel runner. test_subprocess::acquire() does not serialize
// CWD — it is an 8-slot subprocess semaphore. New ownership tests avoid
// set_current_dir entirely (see show_output_owned_tests.rs).
struct TempCwd {
    previous: PathBuf,
}

impl TempCwd {
    fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { previous }
    }
}

impl Drop for TempCwd {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).unwrap();
    }
}

fn task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: Some("research".to_string()),
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: true,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn read_task_output_uses_persisted_result_file() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let result_path = crate::paths::task_dir("t-result-default").join("result.md");
    std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
    std::fs::write(&result_path, "## Findings\nNo findings.\n").unwrap();

    let output = read_task_output(&task("t-result-default")).unwrap();

    assert_eq!(output, "## Findings\nNo findings.\n");
}

#[test]
fn read_task_output_unwraps_persisted_grok_envelope() {
    let temp = tempfile::tempdir().unwrap();
    let output_path = temp.path().join("grok-output.json");
    let _aid_home = AidHomeGuard::set(temp.path());
    let task = Task {
        agent: AgentKind::Grok,
        output_path: Some(output_path.display().to_string()),
        ..task("t-grok-envelope")
    };
    let envelope = serde_json::json!({
        "text": "# Findings\n\nThe report is rendered markdown."
    });
    std::fs::write(&output_path, serde_json::to_string(&envelope).unwrap()).unwrap();

    let output = read_task_output(&task).unwrap();

    assert_eq!(output, "# Findings\n\nThe report is rendered markdown.");
}

/// Two tasks both declare relative `-o report.md`. Only task A's worktree has the file.
/// Showing task B must never render A's report — even when CWD is A's worktree.
#[test]
fn read_task_output_never_renders_sibling_task_relative_report() {
    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);

    let worktree_a = root.path().join("wt-a");
    let worktree_b = root.path().join("wt-b");
    std::fs::create_dir_all(&worktree_a).unwrap();
    std::fs::create_dir_all(&worktree_b).unwrap();

    let foreign = "FOREIGN_REPORT_TASK_A_ONLY: cursor premium holds run_dispatch_resolve";
    std::fs::write(worktree_a.join("report.md"), foreign).unwrap();
    // Task B never wrote its -o file.

    let task_a = Task {
        worktree_path: Some(worktree_a.display().to_string()), effective_dir: None,
        output_path: Some("report.md".to_string()),
        ..task("t-owner-a")
    };
    let task_b = Task {
        worktree_path: Some(worktree_b.display().to_string()), effective_dir: None,
        output_path: Some("report.md".to_string()),
        ..task("t-victim-b")
    };

    // Caller's CWD is task A's worktree — the pre-fix failure mode.
    let _cwd = TempCwd::enter(&worktree_a);
    let owner = read_task_output(&task_a);
    let victim = read_task_output(&task_b);

    assert_eq!(owner.unwrap(), foreign);
    assert!(
        victim.is_err(),
        "task B has no owned report.md; must not succeed via CWD: {victim:?}"
    );
    if let Ok(leaked) = victim {
        assert!(
            !leaked.contains("FOREIGN_REPORT_TASK_A_ONLY"),
            "task B --output leaked task A's report"
        );
    }
}

/// Relative `-o` must resolve under this task's worktree, not process CWD.
#[test]
fn read_task_output_resolves_relative_report_from_task_worktree() {
    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);
    let worktree = root.path().join("wt");
    let foreign_cwd = root.path().join("cwd");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::create_dir_all(&foreign_cwd).unwrap();
    std::fs::write(worktree.join("report.md"), "owned-by-this-task\n").unwrap();
    std::fs::write(foreign_cwd.join("report.md"), "cwd-foreign-content\n").unwrap();

    let task = Task {
        worktree_path: Some(worktree.display().to_string()), effective_dir: None,
        output_path: Some("report.md".to_string()),
        ..task("t-rel-owned")
    };

    let _cwd = TempCwd::enter(&foreign_cwd);
    let output = read_task_output(&task);

    assert_eq!(output.unwrap(), "owned-by-this-task\n");
}

/// When declared output is missing, --output must say so and use this task's log only.
#[test]
fn output_text_reports_missing_owned_file_then_task_log() {
    use crate::cmd::show::output_text_for_task;
    use crate::store::Store;

    let root = tempfile::tempdir().unwrap();
    let aid_home = root.path().join("aid-home");
    let _aid_home = AidHomeGuard::set(&aid_home);
    let worktree_a = root.path().join("wt-a");
    let worktree_b = root.path().join("wt-b");
    std::fs::create_dir_all(&worktree_a).unwrap();
    std::fs::create_dir_all(&worktree_b).unwrap();
    std::fs::write(
        worktree_a.join("report.md"),
        "FOREIGN_REPORT_TASK_A_ONLY: should never appear for B\n",
    )
    .unwrap();

    let log_b = crate::paths::log_path("t-victim-output");
    std::fs::create_dir_all(log_b.parent().unwrap()).unwrap();
    std::fs::write(&log_b, "task-b-own-log-line\n").unwrap();

    let store = Store::open_memory().unwrap();
    let task_b = Task {
        worktree_path: Some(worktree_b.display().to_string()), effective_dir: None,
        output_path: Some("report.md".to_string()),
        log_path: Some(log_b.display().to_string()),
        ..task("t-victim-output")
    };
    store.insert_task(&task_b).unwrap();

    let _cwd = TempCwd::enter(&worktree_a);
    let text = output_text_for_task(&store, "t-victim-output", true).unwrap();

    assert!(
        !text.contains("FOREIGN_REPORT_TASK_A_ONLY"),
        "foreign report leaked into --output: {text}"
    );
    assert!(
        text.contains("No task-owned output file"),
        "absence must be explicit: {text}"
    );
    assert!(
        text.contains("task-b-own-log-line"),
        "must fall back to this task's log: {text}"
    );
}

// Tests for project runtime state persistence and aggregation.
// Exports: crate-level tests for src/state.rs public APIs.
// Deps: tempfile, chrono, crate::{state, store, types}.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use chrono::{Duration, Local};
use tempfile::TempDir;

use crate::state::{
    compute_state, format_state_summary, load_state, refresh_project_state, save_state,
    state_path, ContextState, HealthState, LearnedState, PerformanceState, ProjectState,
};
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

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

fn cwd_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn state_path_and_roundtrip_use_project_aid_dir() {
    let _lock = cwd_lock().lock().unwrap();
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("repo/src");
    fs::create_dir_all(dir.path().join("repo/.aid")).unwrap();
    fs::create_dir_all(&nested).unwrap();
    let _cwd = TempCwd::enter(&nested);
    let state = sample_state();
    assert!(state_path().unwrap().ends_with(".aid/state.toml"));
    save_state(&state).unwrap();
    assert_eq!(load_state().unwrap(), Some(state));
}

#[test]
fn compute_state_aggregates_project_metrics() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".aid")).unwrap();
    fs::write(dir.path().join(".aid/project.toml"), "[project]\nid = \"alpha\"\n").unwrap();
    Command::new("git").args(["init"]).current_dir(dir.path()).output().unwrap();
    let store = Store::open_memory().unwrap();
    for task in [
        make_task(
            "t-1",
            AgentKind::Codex,
            TaskStatus::Done,
            dir.path(),
            5,
            120.0,
            Some(1.2),
            VerifyStatus::Passed,
        ),
        make_task(
            "t-2",
            AgentKind::Gemini,
            TaskStatus::Failed,
            dir.path(),
            15,
            60.0,
            Some(0.8),
            VerifyStatus::Failed,
        ),
        make_task(
            "t-3",
            AgentKind::Codex,
            TaskStatus::Merged,
            dir.path(),
            25,
            180.0,
            None,
            VerifyStatus::Skipped,
        ),
        make_task(
            "t-4",
            AgentKind::Codex,
            TaskStatus::Failed,
            dir.path(),
            35,
            90.0,
            Some(0.5),
            VerifyStatus::Failed,
        ),
        make_task(
            "t-5",
            AgentKind::Codex,
            TaskStatus::Done,
            dir.path(),
            45,
            75.0,
            Some(0.3),
            VerifyStatus::Passed,
        ),
    ] {
        store.insert_task(&task).unwrap();
    }
    let state = compute_state(&store, &dir.path().to_string_lossy()).unwrap();
    assert_eq!(state.health.total_tasks, 5);
    assert!((state.health.recent_success_rate - 0.6).abs() < f64::EPSILON);
    assert_eq!(state.performance.best_agent.as_deref(), Some("codex"));
    assert_eq!(state.context.last_task_id.as_deref(), Some("t-1"));
    assert_eq!(state.health.last_verify_status.as_deref(), None);
    assert_eq!(state.learned.effective_tools, Vec::<String>::new());
}

#[test]
fn refresh_project_state_writes_state_after_completion_update() {
    let _lock = cwd_lock().lock().unwrap();
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".aid")).unwrap();
    fs::write(dir.path().join(".aid/project.toml"), "[project]\nid = \"alpha\"\n").unwrap();
    let _cwd = TempCwd::enter(dir.path());
    let store = Store::open_memory().unwrap();
    let mut task = make_task(
        "t-refresh",
        AgentKind::Codex,
        TaskStatus::Pending,
        dir.path(),
        5,
        120.0,
        Some(1.2),
        VerifyStatus::Skipped,
    );
    task.completed_at = None;
    task.duration_ms = None;
    task.cost_usd = None;
    store.insert_task(&task).unwrap();
    store
        .update_task_completion(TaskCompletionUpdate {
            id: task.id.as_str(),
            status: TaskStatus::Done,
            tokens: Some(42),
            duration_ms: 12_000,
            model: Some("gpt-5.4"),
            cost_usd: Some(0.5),
            exit_code: Some(0),
        })
        .unwrap();

    refresh_project_state(&store, &task.id);

    let state = load_state().unwrap().unwrap();
    assert_eq!(state.context.last_task_id.as_deref(), Some("t-refresh"));
    assert_eq!(state.health.total_tasks, 1);
}

#[test]
fn refresh_project_state_skips_tasks_without_repo_path() {
    let _lock = cwd_lock().lock().unwrap();
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".aid")).unwrap();
    fs::write(dir.path().join(".aid/project.toml"), "[project]\nid = \"alpha\"\n").unwrap();
    let _cwd = TempCwd::enter(dir.path());
    let store = Store::open_memory().unwrap();
    let mut task = make_task(
        "t-no-repo",
        AgentKind::Codex,
        TaskStatus::Done,
        dir.path(),
        5,
        120.0,
        Some(1.2),
        VerifyStatus::Skipped,
    );
    task.repo_path = None;
    store.insert_task(&task).unwrap();

    refresh_project_state(&store, &task.id);

    assert_eq!(load_state().unwrap(), None);
}

#[test]
fn summary_formats_key_lines() {
    let _lock = cwd_lock().lock().unwrap();
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".aid")).unwrap();
    fs::write(dir.path().join(".aid/project.toml"), "[project]\nid = \"alpha\"\n").unwrap();
    let _cwd = TempCwd::enter(dir.path());
    let summary = format_state_summary(&sample_state());
    assert!(summary.contains("[Project State: alpha]"));
    assert!(summary.contains("Health: 94% success (47/50 recent), verify: passed"));
    assert!(summary.contains("Best agents: codex (92%), gemini (88%)"));
    assert!(summary.contains("Last task: t-abcd (codex)"));
}

fn sample_state() -> ProjectState {
    ProjectState {
        last_updated: Local::now().to_rfc3339(),
        health: HealthState {
            last_verify_status: Some("passed".to_string()),
            last_verify_time: Some(Local::now().to_rfc3339()),
            recent_success_rate: 0.94,
            total_tasks: 50,
        },
        performance: PerformanceState {
            best_agent: Some("codex".to_string()),
            agent_success_rates: [
                ("codex".to_string(), 0.92),
                ("gemini".to_string(), 0.88),
            ]
            .into_iter()
            .collect(),
            avg_task_duration_secs: Some(120.0),
            avg_task_cost_usd: Some(1.5),
        },
        context: ContextState {
            last_task_id: Some("t-abcd".to_string()),
            last_task_agent: Some("codex".to_string()),
            active_branch: Some("main".to_string()),
        },
        learned: LearnedState {
            effective_tools: Vec::new(),
            common_failure_patterns: Vec::new(),
        },
    }
}

fn make_task(
    id: &str,
    agent: AgentKind,
    status: TaskStatus,
    repo_path: &Path,
    minutes_ago: i64,
    duration_secs: f64,
    cost_usd: Option<f64>,
    verify_status: VerifyStatus,
) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo_path.to_string_lossy().to_string()),
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some((duration_secs * 1000.0) as i64),
        model: None,
        cost_usd,
        exit_code: None,
        created_at: Local::now() - Duration::minutes(minutes_ago),
        completed_at: Some(Local::now() - Duration::minutes(minutes_ago - 1)),
        verify: None,
        verify_status,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

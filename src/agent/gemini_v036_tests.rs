// Gemini v0.36 adapter tests for new CLI flags and event shapes.
// Covers: include-directories, auto-routing, milestone events, and quota detection.

use super::*;
use crate::agent::{Agent, RunOpts};
use crate::paths::AidHomeGuard;
use tempfile::TempDir;

#[test]
fn build_command_includes_external_context_directories() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let shared_dir = temp.path().join("shared");
    std::fs::create_dir_all(&repo).unwrap();
    let opts = RunOpts {
        dir: Some(repo.to_string_lossy().into_owned()),
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![
            repo.join("src/main.rs").to_string_lossy().into_owned(),
            shared_dir.join("a.md").to_string_lossy().into_owned(),
            shared_dir.join("b.md").to_string_lossy().into_owned(),
            "../sibling/config.toml".to_string(),
        ],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = GeminiAgent.build_command("test", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let include_dirs: Vec<&str> = args
        .windows(2)
        .filter_map(|pair| (pair[0] == "--include-directories").then_some(pair[1].as_str()))
        .collect();
    let shared_dir = shared_dir.to_string_lossy().into_owned();
    assert_eq!(include_dirs.len(), 2);
    assert!(include_dirs.contains(&"../sibling"));
    assert!(include_dirs.contains(&shared_dir.as_str()));
}

#[test]
fn build_command_skips_default_model_for_auto_routing() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = GeminiAgent.build_command("test", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(!args.iter().any(|arg| arg == "-m"));
}

#[test]
fn parses_skill_and_hook_events_as_milestones() {
    let task_id = TaskId::generate();
    let skill = serde_json::json!({
        "type": "skill_execute",
        "name": "repo-map",
        "status": "running"
    });
    let hook = serde_json::json!({
        "type": "hook",
        "message": "Post-action checks passed"
    });
    let skill_event = parse_stream_event(&task_id, &skill, Local::now()).unwrap();
    let hook_event = parse_stream_event(&task_id, &hook, Local::now()).unwrap();
    assert_eq!(skill_event.event_kind, EventKind::Milestone);
    assert_eq!(skill_event.detail, "skill repo-map: running");
    assert_eq!(hook_event.event_kind, EventKind::Milestone);
    assert_eq!(hook_event.detail, "hook: Post-action checks passed");
}

#[test]
fn parses_gemini_rate_limit_errors() {
    let temp = TempDir::new().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let task_id = TaskId::generate();
    let json = serde_json::json!({
        "type": "error",
        "message": "resourceExhausted: RATE_LIMIT_EXCEEDED"
    });
    let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(crate::rate_limit::is_rate_limited(&AgentKind::Gemini));
}

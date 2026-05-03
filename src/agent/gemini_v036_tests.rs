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
fn build_command_sets_trust_workspace_env_by_default() {
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
    // Ensure no inherited override leaks in from the test environment.
    // Safety: tests in this file run single-threaded via cargo's default
    // harness; we restore the previous value on drop below.
    struct EnvGuard {
        prev: Option<std::ffi::OsString>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // Safety: setting/removing a process env var is unsafe in
            // multi-threaded contexts; cargo test runs each test on its own
            // thread but they may share process env. The override window is
            // narrow and the value is restored deterministically.
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var("GEMINI_CLI_TRUST_WORKSPACE", v),
                    None => std::env::remove_var("GEMINI_CLI_TRUST_WORKSPACE"),
                }
            }
        }
    }
    let _guard = EnvGuard {
        prev: std::env::var_os("GEMINI_CLI_TRUST_WORKSPACE"),
    };
    // Safety: see EnvGuard above.
    unsafe {
        std::env::remove_var("GEMINI_CLI_TRUST_WORKSPACE");
    }

    let cmd = GeminiAgent.build_command("test", &opts).unwrap();
    let env_pair = cmd
        .get_envs()
        .find(|(key, _)| *key == std::ffi::OsStr::new("GEMINI_CLI_TRUST_WORKSPACE"));
    let value = env_pair
        .and_then(|(_, v)| v)
        .map(|v| v.to_string_lossy().into_owned());
    assert_eq!(value.as_deref(), Some("true"));
}

#[test]
fn build_command_respects_pre_existing_trust_workspace_override() {
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
    struct EnvGuard {
        prev: Option<std::ffi::OsString>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // Safety: see other EnvGuard in this file.
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var("GEMINI_CLI_TRUST_WORKSPACE", v),
                    None => std::env::remove_var("GEMINI_CLI_TRUST_WORKSPACE"),
                }
            }
        }
    }
    let _guard = EnvGuard {
        prev: std::env::var_os("GEMINI_CLI_TRUST_WORKSPACE"),
    };
    // Safety: see EnvGuard.
    unsafe {
        std::env::set_var("GEMINI_CLI_TRUST_WORKSPACE", "false");
    }

    let cmd = GeminiAgent.build_command("test", &opts).unwrap();
    let explicit = cmd
        .get_envs()
        .find(|(key, _)| *key == std::ffi::OsStr::new("GEMINI_CLI_TRUST_WORKSPACE"));
    // When the caller already has the var set, we must NOT clobber it via
    // Command::env — the child should inherit the existing value.
    assert!(explicit.is_none());
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

#[test]
fn extract_response_handles_content_arrays_and_tool_boundaries() {
    let output = r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Alpha"}],"delta":true}
{"type":"message","role":"assistant","content":[{"type":"text","text":" beta"}],"delta":true}
{"type":"tool_call","name":"Read","arguments":{"file":"src/main.rs"}}
{"type":"message","role":"assistant","content":[{"type":"text","text":"Gamma"}],"delta":true}
{"type":"result","status":"success"}"#;

    let result = extract_response(output);

    assert_eq!(result, Some("Alpha beta\n\nGamma".to_string()));
}

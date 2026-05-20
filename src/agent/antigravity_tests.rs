// Tests for the Antigravity CLI adapter command shape and plain-text completion.
// Exports: module-scoped tests only.
// Deps: super::AntigravityAgent, crate::agent::Agent, tempfile.

use super::{agy_include_directories, AntigravityAgent};
use crate::agent::{Agent, RunOpts};
use crate::types::{AgentKind, TaskId, TaskStatus};
use tempfile::tempdir;

fn opts(read_only: bool, context_files: Vec<String>) -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only,
        context_files,
        session_id: None,
        env: None,
        env_forward: None,
    }
}

fn args_for(opts: &RunOpts) -> Vec<String> {
    AntigravityAgent
        .build_command("test prompt", opts)
        .unwrap()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn build_command_uses_agy_print_mode_and_skip_permissions() {
    let cmd = AntigravityAgent
        .build_command("test prompt", &opts(false, vec![]))
        .unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert_eq!(cmd.get_program().to_string_lossy(), "agy");
    assert!(args.windows(2).any(|pair| pair == ["-p", "test prompt"]));
    assert!(args.windows(2).any(|pair| pair == ["--print-timeout", "24h"]));
    assert!(args.iter().any(|arg| arg == "--dangerously-skip-permissions"));
}

#[test]
fn build_command_errors_when_read_only_requested() {
    let err = AntigravityAgent
        .build_command("test prompt", &opts(true, vec![]))
        .unwrap_err();

    assert!(err.to_string().contains("read-only"));
}

#[test]
fn context_files_dedupe_shared_parent() {
    let context_files = vec!["src/one.rs".to_string(), "src/two.rs".to_string()];
    let args = args_for(&opts(false, context_files));

    assert_eq!(args.iter().filter(|arg| arg.as_str() == "--add-dir").count(), 1);
    assert!(args.windows(2).any(|pair| pair == ["--add-dir", "src"]));
}

#[test]
fn context_files_include_distinct_parent_dirs() {
    let context_files = vec!["src/one.rs".to_string(), "tests/two.rs".to_string()];
    let args = args_for(&opts(false, context_files));

    assert_eq!(args.iter().filter(|arg| arg.as_str() == "--add-dir").count(), 2);
    assert!(args.windows(2).any(|pair| pair == ["--add-dir", "src"]));
    assert!(args.windows(2).any(|pair| pair == ["--add-dir", "tests"]));
}

#[test]
fn context_entry_that_is_directory_is_used_as_is() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_string_lossy().to_string();
    let args = args_for(&opts(false, vec![path.clone()]));

    assert!(args.windows(2).any(|pair| pair == ["--add-dir", path.as_str()]));
}

#[test]
fn include_directories_adds_run_dir_and_sorts() {
    let dirs = agy_include_directories(Some("workspace"), &["src/main.rs".to_string()]);

    assert_eq!(dirs, vec!["src".to_string(), "workspace".to_string()]);
}

#[test]
fn streaming_is_false_and_parse_event_returns_none() {
    let task_id = TaskId::generate();

    assert!(!AntigravityAgent.streaming());
    assert!(AntigravityAgent.parse_event(&task_id, "anything").is_none());
}

#[test]
fn parse_completion_returns_plain_done_status_for_empty_output() {
    let completion = AntigravityAgent.parse_completion("  \n");

    assert_eq!(completion.tokens, None);
    assert_eq!(completion.model, None);
    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.cost_usd, None);
    assert_eq!(completion.exit_code, None);
}

#[test]
fn kind_returns_antigravity() {
    assert_eq!(AntigravityAgent.kind(), AgentKind::Antigravity);
}

// Unit tests for the MiMo Code adapter (build_command flags + event parsing).

use super::super::Agent;
use super::*;
use crate::{paths, rate_limit};

fn base_opts() -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}

fn args_of(prompt: &str, opts: &RunOpts) -> Vec<String> {
    MiMoCodeAgent
        .build_command(prompt, opts)
        .expect("command should build")
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect()
}

#[test]
fn build_command_includes_mimocode_permission_flag() {
    let cmd = MiMoCodeAgent
        .build_command("test prompt", &base_opts())
        .expect("command should build");
    assert_eq!(cmd.get_program().to_string_lossy(), "mimo");
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
    assert_eq!(args.first().map(String::as_str), Some("run"));
    assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    assert!(!args.contains(&"--auto".to_string()));
    assert!(args.contains(&"--format".to_string()));
    assert!(args.contains(&"json".to_string()));
    assert!(args.contains(&"--thinking".to_string()));
}

#[test]
fn build_command_defaults_to_auto_model_when_unset() {
    // MiMo's own CLI default is server-rejected, so we must inject a valid one.
    let args = args_of("test", &base_opts());
    assert!(args.windows(2).any(|pair| pair == ["-m", "mimo/mimo-auto"]));
}

#[test]
fn build_command_uses_explicit_model_over_default() {
    let opts = RunOpts { model: Some("nvidia/moonshotai/kimi-k2.6".to_string()), ..base_opts() };
    let args = args_of("test", &opts);
    assert!(args.windows(2).any(|pair| pair == ["-m", "nvidia/moonshotai/kimi-k2.6"]));
    assert!(!args.contains(&"mimo/mimo-auto".to_string()));
}

#[test]
fn build_command_includes_session_flags() {
    let opts = RunOpts { session_id: Some("ses_abc".to_string()), ..base_opts() };
    let args = args_of("test", &opts);
    assert!(args.contains(&"--session".to_string()));
    assert!(args.contains(&"ses_abc".to_string()));
    assert!(args.contains(&"--continue".to_string()));
    assert!(args.contains(&"--fork".to_string()));
}

#[test]
fn build_command_includes_context_files() {
    let opts = RunOpts { context_files: vec!["src/main.rs".to_string()], ..base_opts() };
    let args = args_of("test", &opts);
    assert!(args.contains(&"-f".to_string()));
    assert!(args.contains(&"src/main.rs".to_string()));
}

#[test]
fn build_command_sets_current_dir_when_dir_provided() {
    let opts = RunOpts { dir: Some("/tmp/wt".to_string()), ..base_opts() };
    let cmd = MiMoCodeAgent.build_command("test", &opts).expect("command should build");
    assert_eq!(cmd.get_current_dir().expect("dir should be set"), std::path::Path::new("/tmp/wt"));
}

#[test]
fn build_command_sets_minimal_variant_in_budget_mode() {
    let opts = RunOpts { budget: true, ..base_opts() };
    let args = args_of("test", &opts);
    assert!(args.windows(2).any(|pair| pair == ["--variant", "minimal"]));
}

#[test]
fn build_command_read_only_with_result_file_uses_exception_prefix() {
    let opts = RunOpts { result_file: Some("result.md".to_string()), read_only: true, ..base_opts() };
    let args = args_of("inspect", &opts);
    let last_arg = args.last().expect("should have prompt as last arg");
    assert!(last_arg.contains("EXCEPT the result file specified in this prompt"));
}

#[test]
fn build_command_read_only_without_result_file_keeps_strict_prefix() {
    let opts = RunOpts { read_only: true, ..base_opts() };
    let args = args_of("inspect", &opts);
    let last_arg = args.last().expect("should have prompt as last arg");
    assert!(last_arg.contains("Do NOT modify, create, or delete any files. Only read and analyze."));
}

#[test]
fn parse_event_marks_mimocode_rate_limits() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    rate_limit::clear_rate_limit(&AgentKind::MiMoCode);
    let event = MiMoCodeAgent
        .parse_event(&TaskId("t-mimocode".to_string()), r#"{"type":"error","message":"rate limit exceeded"}"#)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(rate_limit::is_rate_limited(&AgentKind::MiMoCode));
    rate_limit::clear_rate_limit(&AgentKind::MiMoCode);
}

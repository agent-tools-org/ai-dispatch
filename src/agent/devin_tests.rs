// Tests for the Devin adapter: launch flags, read-only mapping, hooks, model parsing.
// Captured CLI output is inlined; no test runs the real `devin` binary.

use super::*;
use crate::agent::Agent;

fn opts(read_only: bool) -> RunOpts {
    RunOpts {
        dir: Some("/tmp".to_string()),
        model: Some("swe-2-high".to_string()),
        read_only,
        ..Default::default()
    }
}

fn args_for(opts: &RunOpts) -> Vec<String> {
    DevinAgent
        .build_command("-starts with a dash", opts)
        .expect("build command")
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2).any(|pair| pair[0] == flag && pair[1] == value)
}

#[test]
fn build_command_runs_print_mode_in_process_without_trust_prompt() {
    let cmd = DevinAgent.build_command("hello", &opts(false)).expect("build");
    assert_eq!(cmd.get_program().to_string_lossy(), "devin");
    assert_eq!(cmd.get_current_dir(), Some(Path::new("/tmp")));
    let args = args_for(&opts(false));
    assert!(args.contains(&"--print=-starts with a dash".to_string()));
    assert!(has_pair(&args, "--permission-mode", "dangerous"));
    assert!(has_pair(&args, "--respect-workspace-trust", "false"));
    assert!(has_pair(&args, "--process-model", "in-process"));
    assert!(has_pair(&args, "--model", "swe-2-high"));
    assert!(!args.iter().any(|arg| arg == "--config"), "no watched log, no hook config");
}

#[test]
fn read_only_uses_auto_permission_mode() {
    let args = args_for(&opts(true));
    assert!(has_pair(&args, "--permission-mode", "auto"));
}

#[test]
fn read_only_with_result_file_keeps_writes_and_prompt_contract() {
    let mut opts = opts(true);
    opts.result_file = Some("/tmp/result.md".to_string());
    let args = args_for(&opts);
    assert!(has_pair(&args, "--permission-mode", "dangerous"));
    let prompt = args.iter().find(|arg| arg.starts_with("--print=")).expect("prompt");
    assert_ne!(prompt, "--print=-starts with a dash", "read-only prompt contract must wrap it");
}

#[test]
fn missing_workspace_is_rejected() {
    let mut opts = opts(false);
    opts.dir = Some("/definitely/not/a/dir".to_string());
    assert!(DevinAgent.build_command("hello", &opts).is_err());
}

#[test]
fn print_mode_is_buffered_and_takes_no_input() {
    assert!(!DevinAgent.streaming());
    assert!(!DevinAgent.accepts_interactive_input());
    assert!(!DevinAgent.accepts_idle_nudge());
    assert!(!AgentKind::Devin.supports_session_resume());
}

#[test]
fn liveness_hooks_keep_user_settings_and_existing_hooks() {
    let user = json!({
        "agent": { "model": "opus" },
        "hooks": { "PreToolUse": [{ "matcher": "exec", "hooks": [] }] }
    });
    let merged = with_liveness_hooks(user, Path::new("/aid/tasks/t-1/agent.log")).expect("merge");
    assert_eq!(merged["agent"]["model"], "opus");
    let pre = merged["hooks"]["PreToolUse"].as_array().expect("pre hooks");
    assert_eq!(pre.len(), 2, "user hook kept, liveness hook appended");
    assert_eq!(pre[0]["matcher"], "exec");
    let command = pre[1]["hooks"][0]["command"].as_str().expect("command");
    assert_eq!(command, "cat >/dev/null; echo \"devin PreToolUse\" >> '/aid/tasks/t-1/agent.log'");
    assert!(merged["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .is_some_and(|text| text.contains("devin PostToolUse")));
}

#[test]
fn liveness_hooks_refuse_unquotable_paths_and_non_object_configs() {
    assert!(with_liveness_hooks(json!({}), Path::new("/tmp/it's/agent.log")).is_none());
    assert!(with_liveness_hooks(json!([]), Path::new("/tmp/agent.log")).is_none());
    assert!(with_liveness_hooks(json!({ "hooks": [] }), Path::new("/tmp/agent.log")).is_none());
}

#[test]
fn parses_families_aliases_and_model_ids() {
    let output = "Available models (53 families)

Adaptive (adaptive)
  adaptive                       Adaptive  [$0.5 / 1M Input · $2 / 1M Output]

SWE-2 (swe-2)
  aliases: swe
  swe-2-high                     SWE-2 High  [262K context, Free]
  swe-2-medium                   SWE-2 Medium  [262K context, Free]

Claude Opus 5.5 (claude-opus-5.5)
  aliases: opus, claude-opus
  claude-opus-5-5-high           Claude Opus 5.5 High  [1M context, $4 / 1M Input]
";
    assert_eq!(
        parse_devin_models_output(output),
        [
            "adaptive", "swe-2", "swe", "swe-2-high", "swe-2-medium",
            "claude-opus-5.5", "opus", "claude-opus", "claude-opus-5-5-high",
        ]
    );
    assert!(parse_devin_models_output("").is_empty());
}

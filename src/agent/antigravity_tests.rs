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
        sandbox: false,
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
fn build_command_with_sandbox_and_read_only_proceeds() {
    let mut opts = opts(true, vec![]);
    opts.sandbox = true;

    assert!(crate::sandbox::can_sandbox(AgentKind::Antigravity));
    assert!(AntigravityAgent.build_command("test prompt", &opts).is_ok());
}

#[test]
fn build_command_read_only_without_plan_mode_prepends_prompt_prefix() {
    let args = args_for(&opts(true, vec![]));

    let prompt = args
        .windows(2)
        .find(|pair| pair[0] == "-p")
        .map(|pair| pair[1].as_str())
        .unwrap();
    assert!(prompt.starts_with("IMPORTANT: READ-ONLY MODE."));
    assert!(prompt.contains("test prompt"));
}

/// Every value agy receives for `--add-dir` must be absolute; a relative one is rejected
/// outright and poisons the workspace root for the rest of the session.
fn add_dir_values(args: &[String]) -> Vec<String> {
    args.windows(2)
        .filter(|pair| pair[0] == "--add-dir")
        .map(|pair| pair[1].clone())
        .collect()
}

fn opts_in(dir: &str, context_files: Vec<String>) -> RunOpts {
    let mut run_opts = opts(false, context_files);
    run_opts.dir = Some(dir.to_string());
    run_opts
}

#[test]
fn context_files_dedupe_shared_parent() {
    let dirs = add_dir_values(&args_for(&opts_in(
        "/work/repo",
        vec!["src/one.rs".to_string(), "src/two.rs".to_string()],
    )));

    assert_eq!(dirs, vec!["/work/repo".to_string(), "/work/repo/src".to_string()]);
}

#[test]
fn context_files_include_distinct_parent_dirs() {
    let dirs = add_dir_values(&args_for(&opts_in(
        "/work/repo",
        vec!["src/one.rs".to_string(), "tests/two.rs".to_string()],
    )));

    assert_eq!(
        dirs,
        vec![
            "/work/repo".to_string(),
            "/work/repo/src".to_string(),
            "/work/repo/tests".to_string(),
        ]
    );
}

#[test]
fn relative_run_dir_is_absolutized_before_reaching_agy() {
    let dirs = add_dir_values(&args_for(&opts_in(".", vec![])));
    let cwd = std::env::current_dir().unwrap().to_string_lossy().into_owned();

    assert_eq!(dirs, vec![cwd]);
}

#[test]
fn bare_context_filename_maps_to_run_dir() {
    let dirs = add_dir_values(&args_for(&opts_in("/work/repo", vec!["notes.md".to_string()])));

    assert_eq!(dirs, vec!["/work/repo".to_string()]);
}

#[test]
fn every_add_dir_value_is_absolute() {
    let args = args_for(&opts_in(
        ".",
        vec!["src/one.rs".to_string(), "notes.md".to_string()],
    ));

    let dirs = add_dir_values(&args);
    assert!(!dirs.is_empty());
    for dir in dirs {
        assert!(
            std::path::Path::new(&dir).is_absolute(),
            "agy rejects non-absolute --add-dir, got {dir}"
        );
    }
}

#[test]
fn context_entry_that_is_directory_is_used_as_is() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_string_lossy().to_string();
    // Symlinked temp roots (/var -> /private/var on macOS) resolve to their real path.
    let canonical = std::fs::canonicalize(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let args = args_for(&opts(false, vec![path]));

    assert_eq!(add_dir_values(&args), vec![canonical]);
}

#[test]
fn include_directories_adds_run_dir_and_sorts() {
    let dirs = agy_include_directories(
        Some(std::path::Path::new("/work/workspace")),
        &["src/main.rs".to_string()],
    );

    assert_eq!(
        dirs,
        vec!["/work/workspace".to_string(), "/work/workspace/src".to_string()]
    );
}

#[test]
fn capabilities_prefer_mode_flag_when_approval_mode_is_absent() {
    let help = "  --mode  Set the agent execution mode for this session (accept-edits, plan)\n  \
                --model  Model for the current CLI session\n";
    let caps = super::parse_agy_capabilities(help);

    assert_eq!(caps.plan_mode_flag, Some("--mode"));
    assert!(caps.has_model_flag);
}

#[test]
fn capabilities_prefer_approval_mode_when_present() {
    let caps = super::parse_agy_capabilities("  --approval-mode plan\n  --mode x\n");

    assert_eq!(caps.plan_mode_flag, Some("--approval-mode"));
}

#[test]
fn capabilities_report_no_plan_mode_when_cli_has_neither() {
    let caps = super::parse_agy_capabilities("  --print\n  --add-dir\n");

    assert_eq!(caps.plan_mode_flag, None);
    assert!(!caps.has_model_flag);
}

#[test]
fn model_is_passed_with_long_flag_only() {
    // agy has no `-m` short alias; passing one aborts the CLI with "flags provided but not defined".
    let mut run_opts = opts(false, vec![]);
    run_opts.model = Some("gemini-3-pro".to_string());
    let args = args_for(&run_opts);

    assert!(!args.iter().any(|arg| arg == "-m"));
}

#[test]
fn streaming_is_false_and_parse_event_returns_none() {
    let task_id = TaskId::generate();

    assert!(!AntigravityAgent.streaming());
    assert!(AntigravityAgent.parse_event(&task_id, "anything").is_none());
}

#[test]
fn parse_completion_emits_unknown_model_and_cost() {
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

/// agy is a print-mode CLI: nothing reaches stdout until a turn completes, so aid's
/// liveness check has to read a file agy actually writes while it works. Proven on
/// t-7fbbd0e7 (2026-08-08): agy's own log carried 24KB and three streamGenerateContent
/// calls while aid recorded "no agent output since spawn" and reaped it at 180s.
#[test]
fn build_command_points_agy_log_at_the_task_dir() {
    let built = RunOpts {
        env: Some(std::collections::HashMap::from([(
            crate::agent::AGENT_LOG_ENV.to_string(),
            "/tmp/aid-test/tasks/t-abcd1234/agent.log".to_string(),
        )])),
        ..opts(false, vec![])
    };
    let cmd = AntigravityAgent.build_command("do the thing", &built).unwrap();
    let args: Vec<String> =
        cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let idx = args.iter().position(|a| a == "--log-file").expect("--log-file must be passed");
    let path = &args[idx + 1];
    assert!(path.contains("t-abcd1234"), "log must be per task: {path}");
    assert!(path.ends_with("agent.log"), "and named for aid's watcher: {path}");
}

/// Without a task id there is nothing to name the file after; passing a bare flag
/// would be worse than passing none.
#[test]
fn build_command_omits_the_log_flag_when_no_task_is_known() {
    let cmd = AntigravityAgent.build_command("do the thing", &opts(false, vec![])).unwrap();
    let args: Vec<String> =
        cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(!args.iter().any(|a| a == "--log-file"), "got: {args:?}");
}

/// The adapter carries no isolation policy: it passes the log it was handed and nothing
/// otherwise. Whether the path is watchable is decided where the wrapping is known —
/// see `env_with_agent_log` and its callers.
#[test]
fn build_command_passes_only_the_log_it_was_handed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("ensure dirs");

    let none = crate::agent::env_with_agent_log(None, "t-abcd1234", false);
    assert!(none.is_none(), "an unwatchable run must be handed no path");

    let seeded = crate::agent::env_with_agent_log(None, "t-abcd1235", true).unwrap();
    let path = seeded.get(crate::agent::AGENT_LOG_ENV).expect("seeded when watchable");
    assert!(path.contains("t-abcd1235") && path.ends_with("agent.log"), "got: {path}");
}

#[test]
fn parse_agy_models_output_ignores_stderr_noise_and_error_lines() {
    let input = "\
Fetching available models...
ERROR: failed to ping telemetry service
error: network latency high
[ERROR] connection pool exhausted
gemini-3.7-flash-high\tGemini 3.7 Flash (High)
gemini-3.6-flash-high\tGemini 3.6 Flash (High)
";
    let models = super::parse_agy_models_output(input);
    assert!(!models.contains(&"ERROR".to_string()), "ERROR line must not be parsed as model");
    assert!(!models.contains(&"error".to_string()), "error line must not be parsed as model");
    assert_eq!(models, vec!["gemini-3.7-flash-high", "gemini-3.6-flash-high"]);
}

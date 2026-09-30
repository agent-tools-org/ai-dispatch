// Tests for OpenCodeOverlayAgent and parity across OpenCode family adapters.
// Asserts identical argv and event parsing for OpenCode, Kilo, MiMoCode, and Custom.

use super::*;
use crate::agent::read_only::read_only_prompt;
use crate::{paths, rate_limit};
use std::process::Command;

fn base_opts() -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: Vec::new(),
        session_id: None,
        env: None,
        env_forward: None,
    }
}

fn command_args(cmd: &Command) -> Vec<String> {
    cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
}

#[test]
fn overlay_kind_and_model_resolution() {
    let agent = OpenCodeOverlayAgent::new("mimo".into(), "MiMo".into(), "mimo/mimo-v2.5-pro".into());
    assert_eq!(agent.kind(), AgentKind::Custom);

    let cmd_default = agent.build_command("hi", &base_opts()).unwrap();
    let args_default = command_args(&cmd_default);
    let i = args_default.iter().position(|a| a == "-m").expect("-m flag");
    assert_eq!(args_default.get(i + 1).map(String::as_str), Some("mimo/mimo-v2.5-pro"));

    let mut opts = base_opts();
    opts.model = Some("mimo/mimo-v2.5".into());
    let cmd_override = agent.build_command("hi", &opts).unwrap();
    let args_override = command_args(&cmd_override);
    let i2 = args_override.iter().position(|a| a == "-m").expect("-m flag");
    assert_eq!(args_override.get(i2 + 1).map(String::as_str), Some("mimo/mimo-v2.5"));
}

#[test]
fn overlay_spec_uses_binary_args_kind_and_rate_limit_kind() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    rate_limit::clear_rate_limit(&AgentKind::MiMoCode, None);
    rate_limit::clear_rate_limit(&AgentKind::OpenCode, None);
    let agent = OpenCodeOverlayAgent::from_spec(OpenCodeOverlaySpec {
        id: "mimocode".into(),
        display_name: "MiMo Code".into(),
        reported_kind: AgentKind::MiMoCode,
        binary: "mimo".into(),
        extra_args: vec!["--dangerously-skip-permissions".into()],
        default_model: Some("mimo/mimo-auto".into()),
        interactive_input: true,
        rate_limit_kind: AgentKind::MiMoCode,
        allow_external_directories: false,
        probe_served_models: false,
    });
    let cmd = agent.build_command("hi", &base_opts()).unwrap();
    assert_eq!(agent.kind(), AgentKind::MiMoCode);
    assert_eq!(cmd.get_program().to_string_lossy(), "mimo");
    let args = command_args(&cmd);
    assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    assert!(args.windows(2).any(|pair| pair == ["-m", "mimo/mimo-auto"]));
    let event = agent
        .parse_event(
            &TaskId("t-mimo".into()),
            r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here"}}}"#,
        )
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(rate_limit::is_rate_limited(&AgentKind::MiMoCode, None));
    assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
    rate_limit::clear_rate_limit(&AgentKind::MiMoCode, None);
    rate_limit::clear_rate_limit(&AgentKind::OpenCode, None);
}

#[test]
fn custom_overlay_marks_its_own_id_not_opencode() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let agent = OpenCodeOverlayAgent::from_spec(OpenCodeOverlaySpec {
        id: "auditor".into(),
        display_name: "Auditor".into(),
        reported_kind: AgentKind::Custom,
        binary: "opencode".into(),
        extra_args: Vec::new(),
        default_model: Some("x".into()),
        interactive_input: false,
        rate_limit_kind: AgentKind::OpenCode,
        allow_external_directories: true,
        probe_served_models: false,
    });
    assert!(!agent.accepts_interactive_input());
    let _ = agent.parse_event(
        &TaskId("t-aud".into()),
        r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here"}}}"#,
    );
    assert!(rate_limit::is_rate_limited(&AgentKind::Custom, Some("auditor")));
    assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
    assert!(!rate_limit::is_rate_limited(&AgentKind::Custom, Some("other")));
}

#[test]
fn overlay_served_models_probe_behavior() {
    if let Some(home) = std::env::var_os("AID_TEST_OPENCODE_PROBE") {
        let native = crate::agent::get_agent(AgentKind::OpenCode);
        assert_eq!(native.kind(), AgentKind::OpenCode);
        assert_eq!(native.rate_limit_name(), None);
        assert!(native.streaming() && native.accepts_interactive_input() && native.needs_pty());
        assert_eq!(native.default_model(), None);
        assert_eq!(native.served_models().unwrap(), Some(vec!["provider/native".into()]));
        for kind in [AgentKind::Kilo, AgentKind::MiMoCode] {
            assert_eq!(crate::agent::get_agent(kind).served_models().unwrap(), None);
        }
        let custom = OpenCodeOverlayAgent::new("custom".into(), "Custom".into(), "provider/model".into());
        assert_eq!(custom.served_models().unwrap(), None);
        assert_eq!(std::fs::read_to_string(std::path::Path::new(&home).join("calls")).unwrap(), "models\n");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("opencode");
    std::fs::write(&binary, concat!(
        "#!/bin/sh\n",
        "printf '%s\\n' \"$*\" >> \"$AID_TEST_OPENCODE_PROBE/calls\"\n",
        "printf 'Fetching available models...\\nprovider/native\\nprovider/native\\n'\n",
    )).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "agent::opencode_overlay::tests::overlay_served_models_probe_behavior", "--nocapture"])
        .env("PATH", temp.path())
        .env("AID_TEST_OPENCODE_PROBE", temp.path())
        .output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn argv_parity_base_opts_across_all_variants() {
    let prompt = "test base prompt";
    let opts = base_opts();

    // Native OpenCode
    let oc_cmd = super::super::get_agent(AgentKind::OpenCode).build_command(prompt, &opts).unwrap();
    assert_eq!(oc_cmd.get_program().to_string_lossy(), "opencode");
    assert_eq!(command_args(&oc_cmd), vec!["run", "--format", "json", "--thinking", prompt]);

    // Kilo
    let kilo_cmd = super::super::kilo::agent().build_command(prompt, &opts).unwrap();
    assert_eq!(kilo_cmd.get_program().to_string_lossy(), "kilo");
    assert_eq!(command_args(&kilo_cmd), vec!["run", "--auto", "--format", "json", "--thinking", prompt]);

    // MiMoCode
    let mimo_cmd = super::super::mimocode::agent().build_command(prompt, &opts).unwrap();
    assert_eq!(mimo_cmd.get_program().to_string_lossy(), "mimo");
    assert_eq!(
        command_args(&mimo_cmd),
        vec!["run", "--dangerously-skip-permissions", "--format", "json", "--thinking", "-m", "mimo/mimo-auto", prompt]
    );

    // Custom Delegate
    let custom = OpenCodeOverlayAgent::new("custom-del".into(), "Custom".into(), "provider/model-x".into());
    let custom_cmd = custom.build_command(prompt, &opts).unwrap();
    assert_eq!(custom_cmd.get_program().to_string_lossy(), "opencode");
    assert_eq!(
        command_args(&custom_cmd),
        vec!["run", "--format", "json", "--thinking", "-m", "provider/model-x", prompt]
    );
}

#[test]
fn argv_parity_complex_opts_across_all_variants() {
    let prompt = "complex test prompt";
    let opts = RunOpts {
        dir: Some("/tmp/workdir".into()),
        output: None,
        result_file: Some("report.md".into()),
        model: Some("vendor/override-model".into()),
        budget: true,
        read_only: true,
        sandbox: false,
        context_files: vec!["a.rs".into(), "b.rs".into()],
        session_id: Some("sess-999".into()),
        env: None,
        env_forward: None,
    };
    let expected_prompt = read_only_prompt(prompt, &opts);
    let expected_suffix = vec![
        "--format", "json",
        "--thinking",
        "--session", "sess-999", "--continue", "--fork",
        "--variant", "minimal",
        "-m", "vendor/override-model",
        "--dir", "/tmp/workdir",
        "-f", "a.rs", "-f", "b.rs",
        &expected_prompt,
    ];

    // Native OpenCode
    let oc_cmd = super::super::get_agent(AgentKind::OpenCode).build_command(prompt, &opts).unwrap();
    assert_eq!(oc_cmd.get_program().to_string_lossy(), "opencode");
    let mut expected_oc = vec!["run"];
    expected_oc.extend(expected_suffix.iter().copied());
    assert_eq!(command_args(&oc_cmd), expected_oc);
    assert_eq!(oc_cmd.get_current_dir().unwrap(), std::path::Path::new("/tmp/workdir"));
    let env_map: std::collections::HashMap<_, _> = oc_cmd.get_envs().collect();
    assert!(env_map.contains_key(std::ffi::OsStr::new("OPENCODE_CONFIG_CONTENT")));

    // Kilo
    let kilo_cmd = super::super::kilo::agent().build_command(prompt, &opts).unwrap();
    assert_eq!(kilo_cmd.get_program().to_string_lossy(), "kilo");
    let mut expected_kilo = vec!["run", "--auto"];
    expected_kilo.extend(expected_suffix.iter().copied());
    assert_eq!(command_args(&kilo_cmd), expected_kilo);
    let kilo_envs: std::collections::HashMap<_, _> = kilo_cmd.get_envs().collect();
    assert!(!kilo_envs.contains_key(std::ffi::OsStr::new("OPENCODE_CONFIG_CONTENT")));

    // MiMoCode
    let mimo_cmd = super::super::mimocode::agent().build_command(prompt, &opts).unwrap();
    assert_eq!(mimo_cmd.get_program().to_string_lossy(), "mimo");
    let mut expected_mimo = vec!["run", "--dangerously-skip-permissions"];
    expected_mimo.extend(expected_suffix.iter().copied());
    assert_eq!(command_args(&mimo_cmd), expected_mimo);

    // Custom Delegate
    let custom = OpenCodeOverlayAgent::new("custom-del".into(), "Custom".into(), "provider/default-model".into());
    let custom_cmd = custom.build_command(prompt, &opts).unwrap();
    assert_eq!(custom_cmd.get_program().to_string_lossy(), "opencode");
    let mut expected_custom = vec!["run"];
    expected_custom.extend(expected_suffix.iter().copied());
    assert_eq!(command_args(&custom_cmd), expected_custom);
}

#[test]
fn recorded_stream_event_parsing_parity() {
    let task_id = TaskId("t-stream-proof".into());
    let stream_lines = [
        (r#"{"type":"step_start"}"#, None),
        (
            r#"{"type":"message","content":"Analyzing files...","sessionID":"ses_proof_1"}"#,
            Some(EventKind::Reasoning),
        ),
        (
            r#"{"type":"tool_call","name":"bash","arguments":"cargo test"}"#,
            Some(EventKind::Test),
        ),
        (
            r#"{"type":"tool_call","name":"read","arguments":"src/lib.rs"}"#,
            Some(EventKind::ToolCall),
        ),
        (
            r#"{"type":"auto_compact","message":"compacted turns"}"#,
            Some(EventKind::Milestone),
        ),
        (
            r#"{"type":"step_finish","part":{"tokens":{"total":500,"input":400,"output":100},"cost":0.002}}"#,
            Some(EventKind::Completion),
        ),
        (
            r#"{"type":"completion","tokens":500}"#,
            Some(EventKind::Completion),
        ),
        (
            "Compiling test-crate v0.1.0",
            Some(EventKind::Build),
        ),
    ];

    let oc = super::super::get_agent(AgentKind::OpenCode);
    for (line, expected_kind) in stream_lines {
        let event = oc.parse_event(&task_id, line);
        assert_eq!(event.as_ref().map(|e| e.event_kind), expected_kind, "line: {line}");
        if let Some(ev) = event {
            assert_eq!(ev.task_id, task_id);
            assert!(!ev.detail.is_empty());
        }
    }
}

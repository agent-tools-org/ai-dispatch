// Extracted configuration regression tests.
// Deps: parent module and isolated test fixtures.

use super::super::Agent;
use super::*;
use crate::types::{EventKind, TaskId};

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

fn base_config(command: &str) -> CustomAgentConfig {
    CustomAgentConfig {
        id: "custom".into(),
        display_name: "Custom Agent".into(),
        command: command.into(),
        prompt_mode: default_prompt_mode(),
        prompt_flag: String::new(),
        dir_flag: String::new(),
        model_flag: String::new(),
        output_flag: String::new(),
        fixed_args: Vec::new(),
        streaming: false,
        interactive_input: default_interactive_input(),
        output_format: default_output_format(),
        capabilities: CapabilityScores::default(),
        trust_tier: default_trust_tier(),
        base_url: None,
        provider: None,
        metering: None,
        strengths: Vec::new(),
        delegate_to: None,
        forced_model: None,
        binary: None,
        extra_args: Vec::new(),
        rate_limit_kind: None,
    }
}

#[test]
fn parse_minimal_config() {
    let toml_data = r#"
        [agent]
        id = "aider"
        display_name = "Aider"
        command = "aider"
    "#;
    let config = parse_config(toml_data).unwrap();
    assert_eq!(config.id, "aider");
    assert_eq!(config.prompt_mode, "arg");
    assert_eq!(config.output_format, "text");
    assert!(config.interactive_input);
    assert_eq!(config.capabilities.simple_edit, 0);
}

#[test]
fn parse_full_config() {
    let toml_data = r#"
        [agent]
        id = "example-agent"
        display_name = "Example Agent"
        command = "example-agent-cli"
        prompt_mode = "flag"
        prompt_flag = "--input"
        dir_flag = "--dir"
        model_flag = "--model"
        output_flag = "--out"
        fixed_args = ["--yes", "--batch"]
        streaming = true
        interactive_input = false
        output_format = "jsonl"

        [agent.capabilities]
        research = 2
        simple_edit = 4
        complex_impl = 8
        frontend = 1
        debugging = 5
        testing = 6
        refactoring = 7
        documentation = 2
    "#;
    let config = parse_config(toml_data).unwrap();
    assert!(config.streaming);
    assert!(!config.interactive_input);
    assert_eq!(config.prompt_mode, "flag");
    assert_eq!(config.capabilities.complex_impl, 8);
    assert_eq!(config.fixed_args.len(), 2);
}

#[test]
fn build_command_with_arg_mode() {
    let mut config = base_config("agent-cli");
    config.fixed_args.push("--yes".into());
    let agent = CustomAgent { config };
    let cmd = agent.build_command("ask", &base_opts()).unwrap();
    let args: Vec<_> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, vec!["--yes", "ask"]);
}

#[test]
fn build_command_with_flag_mode() {
    let mut config = base_config("agent-cli");
    config.prompt_mode = "flag".into();
    config.prompt_flag = "--message".into();
    config.fixed_args.push("--ready".into());
    let agent = CustomAgent { config };
    let args: Vec<_> = agent
        .build_command("prompt", &base_opts())
        .unwrap()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, vec!["--ready", "--message", "prompt"]);
}

#[test]
fn custom_agent_kind_is_custom() {
    let agent = CustomAgent {
        config: base_config("agent-cli"),
    };
    assert_eq!(agent.kind(), AgentKind::Custom);
}

#[test]
fn custom_agent_input_capability_is_independent_of_streaming() {
    let mut config = base_config("agent-cli");
    config.streaming = false;
    config.interactive_input = true;
    assert!(CustomAgent { config }.accepts_interactive_input());

    let mut config = base_config("agent-cli");
    config.streaming = true;
    config.interactive_input = false;
    assert!(!CustomAgent { config }.accepts_interactive_input());
}

#[test]
fn build_command_read_only_prepends_warning_to_prompt() {
    let agent = CustomAgent {
        config: base_config("agent-cli"),
    };
    let mut opts = base_opts();
    opts.read_only = true;
    let cmd = agent.build_command("ask", &opts).unwrap();
    let args: Vec<_> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args.last().unwrap().contains("READ-ONLY MODE"));
    assert!(args.last().unwrap().ends_with("\n\nask"));
}

#[test]
fn build_command_read_only_with_result_file_uses_exception_text() {
    let agent = CustomAgent {
        config: base_config("agent-cli"),
    };
    let mut opts = base_opts();
    opts.read_only = true;
    opts.result_file = Some("result.md".into());
    let cmd = agent.build_command("audit", &opts).unwrap();
    let last = cmd
        .get_args()
        .last()
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap();
    assert!(last.contains("EXCEPT the result file"));
}

#[test]
fn build_command_normal_mode_does_not_mutate_prompt() {
    let agent = CustomAgent {
        config: base_config("agent-cli"),
    };
    let cmd = agent.build_command("hello", &base_opts()).unwrap();
    let args: Vec<_> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, vec!["hello"]);
}

#[test]
fn build_command_with_dir() {
    let mut config = base_config("agent-cli");
    config.dir_flag = "--dir".into();
    let mut opts = base_opts();
    opts.dir = Some("/tmp/work".into());
    let cmd = CustomAgent { config }
        .build_command("prompt", &opts)
        .unwrap();
    let args: Vec<_> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(&args[..2], ["--dir".to_string(), "/tmp/work".to_string()]);
    let current_dir = cmd
        .get_current_dir()
        .map(|p| p.to_string_lossy().into_owned());
    assert_eq!(current_dir, Some("/tmp/work".to_string()));
}

#[test]
fn parse_event_jsonl() {
    let mut config = base_config("agent-cli");
    config.output_format = "jsonl".into();
    let agent = CustomAgent { config };
    let task_id = TaskId("t-0001".into());
    let line = r#"{"type":"completion","message":"done"}"#;
    let event = agent.parse_event(&task_id, line).unwrap();
    assert_eq!(event.event_kind, EventKind::Completion);
    assert_eq!(event.detail, "done");
}

#[test]
fn parse_event_text() {
    let config = base_config("agent-cli");
    let agent = CustomAgent { config };
    let task_id = TaskId("t-0002".into());
    let event = agent.parse_event(&task_id, " step ").unwrap();
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "step");
}

// Kilo delegate spec for the OpenCode-compatible overlay engine.
// Exports agent() for get_agent wiring.
// Depends on opencode_overlay and AgentKind.

use super::opencode_overlay::{OpenCodeOverlayAgent, OpenCodeOverlaySpec};
use crate::types::AgentKind;

pub(crate) fn agent() -> OpenCodeOverlayAgent {
    OpenCodeOverlayAgent::from_spec(spec())
}

pub(crate) fn spec() -> OpenCodeOverlaySpec {
    OpenCodeOverlaySpec {
        id: "kilo".to_string(),
        display_name: "Kilo".to_string(),
        reported_kind: AgentKind::Kilo,
        binary: "kilo".to_string(),
        extra_args: vec!["--auto".to_string()],
        default_model: None,
        rate_limit_kind: AgentKind::Kilo,
        allow_external_directories: false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Agent, RunOpts};
    use super::*;
    use crate::{paths, rate_limit};
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
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        }
    }

    fn args_of(prompt: &str, opts: &RunOpts) -> Vec<String> {
        agent()
            .build_command(prompt, opts)
            .expect("command should build")
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn build_command_uses_kilo_binary_and_auto_flag() {
        let cmd = agent().build_command("test prompt", &base_opts()).unwrap();
        assert_eq!(cmd.get_program().to_string_lossy(), "kilo");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args.first().map(String::as_str), Some("run"));
        assert!(args.contains(&"--auto".to_string()));
        assert!(args.contains(&"--format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--thinking".to_string()));
    }

    #[test]
    fn build_command_includes_session_context_dir_and_budget_flags() {
        let opts = RunOpts {
            session_id: Some("ses_abc".to_string()),
            context_files: vec!["src/main.rs".to_string()],
            dir: Some("/tmp/wt".to_string()),
            budget: true,
            ..base_opts()
        };
        let cmd = agent().build_command("test", &opts).unwrap();
        assert_eq!(cmd.get_current_dir().unwrap(), std::path::Path::new("/tmp/wt"));
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["--session", "ses_abc"]));
        assert!(args.contains(&"--continue".to_string()));
        assert!(args.contains(&"--fork".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--variant", "minimal"]));
        assert!(args.windows(2).any(|pair| pair == ["-f", "src/main.rs"]));
    }

    #[test]
    fn build_command_read_only_prefixes_prompt() {
        let opts = RunOpts { read_only: true, ..base_opts() };
        let args = args_of("inspect", &opts);
        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("Do NOT modify, create, or delete any files."));
    }

    #[test]
    fn kilo_reports_kind_and_needs_pty() {
        assert_eq!(agent().kind(), AgentKind::Kilo);
        assert!(agent().needs_pty());
    }

    #[test]
    fn parse_event_marks_kilo_rate_limits() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        rate_limit::clear_rate_limit(&AgentKind::Kilo);
        rate_limit::clear_rate_limit(&AgentKind::OpenCode);
        let event = agent()
            .parse_event(
                &TaskId("t-kilo".to_string()),
                r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here"}}}"#,
            )
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Error);
        assert!(rate_limit::is_rate_limited(&AgentKind::Kilo));
        assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode));
        rate_limit::clear_rate_limit(&AgentKind::Kilo);
        rate_limit::clear_rate_limit(&AgentKind::OpenCode);
    }
}

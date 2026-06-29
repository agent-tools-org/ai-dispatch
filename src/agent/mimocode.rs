// MiMo Code CLI adapter: thin wrapper over OpenCode-compatible JSON format.
// MiMo Code is an OpenCode-family CLI with identical event streaming.

use anyhow::Result;
use std::process::Command;

use super::opencode::{classify_text_line, extract_tokens_from_output, parse_json_event};
use super::RunOpts;
use crate::types::*;

pub struct MiMoCodeAgent;

impl super::Agent for MiMoCodeAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::MiMoCode
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let effective_prompt = if opts.read_only {
            if opts.result_file.is_some() {
                format!(
                    "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files, EXCEPT the result file specified in this prompt. Only read, analyze, and write your findings to the designated result file.\n\n{}",
                    prompt
                )
            } else {
                format!(
                    "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files. Only read and analyze.\n\n{}",
                    prompt
                )
            }
        } else {
            prompt.to_string()
        };
        let mut cmd = Command::new("mimo");
        cmd.arg("run");
        cmd.args(["--format", "json"]);
        cmd.arg("--thinking");
        cmd.arg("--dangerously-skip-permissions");
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["--session", session_id]);
            cmd.arg("--continue");
            cmd.arg("--fork");
        }
        if opts.budget {
            cmd.args(["--variant", "minimal"]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--dir", dir]);
            cmd.current_dir(dir);
        }
        for file in &opts.context_files {
            cmd.args(["-f", file]);
        }
        cmd.arg(&effective_prompt);
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let now = chrono::Local::now();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let event = if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            parse_json_event(task_id, &v, now)
        } else {
            let (kind, detail) = classify_text_line(trimmed);
            kind.map(|k| TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: k,
                detail: super::truncate::truncate_text(detail, 80),
                metadata: None,
            })
        };
        if let Some(ref ev) = event
            && ev.event_kind == EventKind::Error
            && crate::rate_limit::is_rate_limit_error(&ev.detail)
        {
            crate::rate_limit::mark_rate_limited(&AgentKind::MiMoCode, &ev.detail);
        }
        event
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        let (tokens, cost_usd) = extract_tokens_from_output(output);
        CompletionInfo {
            tokens,
            status: TaskStatus::Done,
            model: None,
            cost_usd,
            exit_code: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Agent;
    use super::*;
    use crate::{paths, rate_limit};

    #[test]
    fn build_command_includes_mimocode_permission_flag() {
        let opts = RunOpts {
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
        };
        let cmd = MiMoCodeAgent
            .build_command("test prompt", &opts)
            .expect("command should build");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(cmd.get_program().to_string_lossy(), "mimo");
        assert_eq!(args.first().map(String::as_str), Some("run"));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(!args.contains(&"--auto".to_string()));
        assert!(args.contains(&"--format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--thinking".to_string()));
    }

    #[test]
    fn build_command_includes_session_flags() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: Some("ses_abc".to_string()),
            env: None,
            env_forward: None,
        };
        let cmd = MiMoCodeAgent
            .build_command("test", &opts)
            .expect("command should build");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--session".to_string()));
        assert!(args.contains(&"ses_abc".to_string()));
        assert!(args.contains(&"--continue".to_string()));
        assert!(args.contains(&"--fork".to_string()));
    }

    #[test]
    fn build_command_includes_context_files() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec!["src/main.rs".to_string()],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = MiMoCodeAgent
            .build_command("test", &opts)
            .expect("command should build");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn build_command_sets_current_dir_when_dir_provided() {
        let opts = RunOpts {
            dir: Some("/tmp/wt".to_string()),
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
        };
        let cmd = MiMoCodeAgent
            .build_command("test", &opts)
            .expect("command should build");
        let dir = cmd.get_current_dir().expect("dir should be set");
        assert_eq!(dir, std::path::Path::new("/tmp/wt"));
    }

    #[test]
    fn build_command_sets_minimal_variant_in_budget_mode() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: true,
            read_only: false,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = MiMoCodeAgent
            .build_command("test", &opts)
            .expect("command should build");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["--variant", "minimal"]));
    }

    #[test]
    fn build_command_read_only_with_result_file_uses_exception_prefix() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: Some("result.md".to_string()),
            model: None,
            budget: false,
            read_only: true,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = MiMoCodeAgent.build_command("inspect", &opts).expect("command should build");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("EXCEPT the result file specified in this prompt"));
    }

    #[test]
    fn build_command_read_only_without_result_file_keeps_strict_prefix() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: true,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = MiMoCodeAgent.build_command("inspect", &opts).expect("command should build");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("Do NOT modify, create, or delete any files. Only read and analyze."));
    }

    #[test]
    fn parse_event_marks_mimocode_rate_limits() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        rate_limit::clear_rate_limit(&AgentKind::MiMoCode);
        let event = MiMoCodeAgent.parse_event(&TaskId("t-mimocode".to_string()), r#"{"type":"error","message":"rate limit exceeded"}"#).unwrap();
        assert_eq!(event.event_kind, EventKind::Error);
        assert!(rate_limit::is_rate_limited(&AgentKind::MiMoCode));
        rate_limit::clear_rate_limit(&AgentKind::MiMoCode);
    }
}

// Codebuff agent adapter that delegates to the aid-codebuff CLI bridge.
// Exports: CodebuffAgent implementing the Agent trait for streaming runs.
// Deps: super::codex for parsing events and crate::types for metadata.

use anyhow::Result;
use std::process::Command;

use super::RunOpts;
use super::codex::CodexAgent;
use crate::types::*;

pub struct CodebuffAgent;

impl super::Agent for CodebuffAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Codebuff
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        if std::env::var("CODEBUFF_API_KEY").unwrap_or_default().is_empty() {
            anyhow::bail!(
                "CODEBUFF_API_KEY not set.\n\
                 1. Get an API key at: https://www.codebuff.com/api-keys\n\
                 2. Export it: export CODEBUFF_API_KEY=cb-pat-...\n\
                 3. Add to your shell profile (~/.zshrc) to persist across sessions"
            );
        }
        let mut cmd = Command::new("aid-codebuff");
        // SDK v0.10+ runs agent locally — needs extra heap for tokenizer + file scanning
        cmd.env("NODE_OPTIONS", "--max-old-space-size=8192");
        let prompt_with_ctx = embed_context_in_prompt(prompt, &opts.context_files)?;
        cmd.arg(&prompt_with_ctx);
        if let Some(ref dir) = opts.dir {
            cmd.args(["--cwd", dir]);
            cmd.current_dir(dir);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        if opts.read_only {
            cmd.arg("--read-only");
        }
        if opts.budget {
            cmd.args(["--mode", "free"]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        CodexAgent.parse_event(task_id, line)
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        CodexAgent.parse_completion(output)
    }
}

fn embed_context_in_prompt(prompt: &str, context_files: &[String]) -> Result<String> {
    if context_files.is_empty() {
        return Ok(prompt.to_string());
    }
    let mut combined = prompt.to_string();
    for file in context_files {
        let contents = std::fs::read_to_string(file)?;
        combined.push_str("\n\n[Context File: ");
        combined.push_str(file);
        combined.push_str("]\n");
        combined.push_str(&contents);
    }
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::CodebuffAgent;
    use crate::agent::{Agent, RunOpts};
    use crate::types::{EventKind, TaskId};
    use std::ffi::{OsStr, OsString};
    use tempfile::NamedTempFile;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    fn parses_codex_compatible_events() {
        let agent = CodebuffAgent;
        let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"Editing src/main.rs"}}"#;
        let event = agent
            .parse_event(&TaskId("t-cb".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert!(event.detail.contains("Editing"));
    }

    #[test]
    fn parses_turn_completed_usage() {
        let agent = CodebuffAgent;
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":50000,"output_tokens":2000,"cached_input_tokens":0},"model":"claude-opus-4-6"}"#;
        let event = agent
            .parse_event(&TaskId("t-cb".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Completion);
    }

    #[test]
    fn build_command_embeds_context_files_in_prompt_arg() {
        let _api_key = EnvVarGuard::set("CODEBUFF_API_KEY", "cb-pat-test");
        let context = NamedTempFile::new().unwrap();
        std::fs::write(context.path(), "extra context").unwrap();
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![context.path().display().to_string()],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodebuffAgent.build_command("base prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args[0],
            format!(
                "base prompt\n\n[Context File: {}]\nextra context",
                context.path().display()
            )
        );
    }
}

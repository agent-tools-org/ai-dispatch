// Droid (Factory.ai) CLI adapter: builds `droid exec` commands and parses streaming JSON events.
// Exports DroidAgent for streaming runs.
// Depends on serde_json for event parsing.

use anyhow::Result;
use std::process::Command;

use super::RunOpts;
use crate::types::*;

mod events;

pub struct DroidAgent;

impl super::Agent for DroidAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Droid
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("droid");
        cmd.args(["exec", "--output-format", "stream-json"]);
        let allow_result = super::read_only::allow_result_file_write(opts);
        if opts.read_only && !allow_result {
            // `--use-spec` is droid's true read-only mode. `--auto low` still
            // permits file creation/modification in non-system directories,
            // so it is NOT a read-only mode despite the name.
            cmd.arg("--use-spec");
        } else {
            // `--auto high` still hits "insufficient permission to proceed.
            // Re-run with --skip-permissions-unsafe" on a wide range of
            // operations in headless aid runs. aid worktrees are sandboxed
            // by branch and the user has opted into autonomous orchestration
            // (parallel to `gemini -y` and `cursor --trust`), so adopt
            // droid's own recommendation. Note: --skip-permissions-unsafe
            // cannot be combined with --auto.
            // With a result file, skip --use-spec so the report can be written.
            cmd.arg("--skip-permissions-unsafe");
        }
        if let Some(ref model) = opts.model {
            let mapped = map_model_name(model);
            cmd.args(["-m", mapped.as_str()]);
        }
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["-s", session_id]);
        }
        // `-f` in droid means "read PROMPT from file" — using it for context
        // files would override the prompt argument. `--append-system-prompt-file`
        // is purpose-built for injecting extra context per file and is repeatable.
        for file in &opts.context_files {
            cmd.args(["--append-system-prompt-file", file]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--cwd", dir]);
            cmd.current_dir(dir);
        }
        let effective_prompt = if allow_result {
            super::read_only::read_only_prompt(prompt, opts)
        } else {
            prompt.to_string()
        };
        cmd.arg(&effective_prompt);
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        events::parse_event(task_id, line)
    }
}

fn map_model_name(model: &str) -> String {
    match model {
        "haiku" => "claude-haiku-4-5-20251001".to_string(),
        "sonnet" => "claude-sonnet-4-6".to_string(),
        // droid's own default is claude-opus-4-7 per `droid exec --help`.
        "opus" => "claude-opus-4-7".to_string(),
        "gpt-4.1-nano" => "gpt-5.4-mini".to_string(),
        "gpt-4.1-mini" => "gpt-5.4-fast".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod model_name_tests {
    use super::map_model_name;

    #[test]
    fn maps_common_shorthand_models() {
        assert_eq!(map_model_name("haiku"), "claude-haiku-4-5-20251001");
        assert_eq!(map_model_name("sonnet"), "claude-sonnet-4-6");
        assert_eq!(map_model_name("opus"), "claude-opus-4-7");
        assert_eq!(map_model_name("gpt-4.1-nano"), "gpt-5.4-mini");
        assert_eq!(map_model_name("gpt-4.1-mini"), "gpt-5.4-fast");
    }

    #[test]
    fn preserves_full_model_ids() {
        assert_eq!(
            map_model_name("claude-haiku-4-5-20251001"),
            "claude-haiku-4-5-20251001"
        );
    }
}

#[cfg(test)]
mod tests;

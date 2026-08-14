// Grok CLI adapter: builds `grok` commands and parses buffered JSON output.
// Probes the `grok` binary specifically — not the generic `agent` name.

use anyhow::{bail, Result};
use chrono::Local;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct GrokAgent;

impl super::Agent for GrokAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Grok
    }

    fn streaming(&self) -> bool {
        false
    }

    fn accepts_interactive_input(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let allow_result = super::read_only::allow_result_file_write(opts);
        let effective_prompt = if allow_result {
            super::read_only::read_only_prompt(&prompt_with_ctx, opts)
        } else {
            prompt_with_ctx
        };
        let mut cmd = Command::new("grok");
        cmd.args(["-p", &effective_prompt, "--output-format", "json"]);
        // Global Claude Code Stop hooks (`hiboss hook stop`) refuse the first
        // exit until `hiboss ask` returns — and `hiboss ask` blocks for a human.
        // Headless `-p` has no human, so the session writes zero bytes and the
        // watchdog reaps it (t-764b2a1d, 1200s). Measured: `--deny Bash(hiboss:*)`
        // lets grok refuse the gate by policy and exit normally. Adapter-local
        // because only this CLI speaks `--deny`; scoped to hiboss (not all Bash)
        // so normal tool use stays intact. The `:*` covers ask/hook/notify.
        cmd.args(["--deny", "Bash(hiboss:*)"]);
        if opts.read_only && !allow_result {
            cmd.args(["--permission-mode", "plan"]);
        } else {
            // Without this grok asks for approval it can never receive: in
            // headless `-p` mode it renders no prompt, it just abandons the tool
            // call, returns `stopReason: "cancelled"` and exits 0. Measured in a
            // scratch repo — "add a line to a.txt" left the file untouched and
            // still billed, and a real 32-turn run burned $1.04 before being
            // cancelled the same way. `--permission-mode auto` is NOT enough; it
            // cancels identically. Every other adapter passes its own form of
            // this (codex `--full-auto`, agy/claude `--dangerously-skip-
            // permissions`, droid `--skip-permissions-unsafe`, kilo `--auto`).
            // Result-file audits need the same write path with prompt-level RO.
            cmd.arg("--always-approve");
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        // grok's --debug-file grows throughout a run, giving both reapers a byte
        // signal for proof of life. Without it, grok is silent on the PTY until
        // exit and looks identical to a dead process to the first-token detector.
        // Mirrors agy's --log-file pattern; the caller decides whether the path
        // is watchable via env_with_agent_log.
        if let Some(log_file) = super::agent_log_from_opts(opts) {
            if let Some(parent) = std::path::Path::new(log_file).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            cmd.args(["--debug-file", log_file]);
        }
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["-r", session_id]);
        }
        if let Some(ref dir) = opts.dir {
            let path = Path::new(dir);
            if !path.is_dir() {
                bail!("Workspace path does not exist: {dir}");
            }
            cmd.args(["--cwd", dir]);
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        parse_grok_completion(output)
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let mut cmd = Command::new("grok");
        cmd.arg("models");
        let Some(output) = super::model_validation::run_probe_cmd(cmd) else {
            return Ok(None);
        };
        let models = parse_grok_models_output(&output.stdout);
        Ok(if models.is_empty() { None } else { Some(models) })
    }
}

pub(crate) fn parse_grok_models_output(output: &str) -> Vec<String> {
    let (mut models, mut in_models_section) = (Vec::new(), false);
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("Available models:") {
            in_models_section = true;
            continue;
        }
        if !in_models_section { continue; }
        let Some(clean) = trimmed.strip_prefix('*').or_else(|| trimmed.strip_prefix('-')) else { continue; };
        let clean = clean.trim();
        let model_name = clean.split_whitespace().next().unwrap_or(clean);
        if !model_name.is_empty() && !models.contains(&model_name.to_string()) {
            models.push(model_name.to_string());
        }
    }
    models
}

#[cfg(test)]
mod parser_tests {
    use super::parse_grok_models_output;

    #[test]
    fn parses_current_grok_models_output_and_ignores_footer_text() {
        let output = r#"You are logged in with grok.com.

Default model: grok-4.6

Available models:
  * grok-4.6 (default)
  - grok-4.5
"#;
        assert_eq!(parse_grok_models_output(output), ["grok-4.6", "grok-4.5"]);
        assert_eq!(parse_grok_models_output(&format!("{output}Footer text")), ["grok-4.6", "grok-4.5"]);
    }
}

/// Find grok's JSON envelope inside a buffer that also carries aid's own writes.
///
/// `finalize_buffered` appends the terminal sentinel to `full_output` *before*
/// calling `parse_completion`, so the buffer is never bare JSON — every grok run
/// failed a whole-buffer parse, not just the nudged ones. An echoed auto-nudge
/// adds a second contaminant, ahead of the envelope instead of after it.
///
/// So candidates are the start of the buffer plus every line beginning with `{`,
/// each parsed with a streaming deserializer that stops at the end of one value
/// and ignores whatever follows. The last value that parses wins; aid's own
/// wording is never matched on.
fn extract_envelope(output: &str) -> Option<Value> {
    let trimmed = output.trim();
    let mut found = first_value(trimmed);
    for (idx, _) in trimmed.match_indices('\n') {
        let rest = trimmed[idx + 1..].trim_start();
        if rest.starts_with('{')
            && let Some(value) = first_value(rest)
        {
            found = Some(value);
        }
    }
    found
}

fn first_value(input: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(input)
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
        .filter(Value::is_object)
}

pub fn extract_response(output: &str) -> Option<String> {
    let value = extract_envelope(output)?;
    value
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

/// The buffer handed here is not grok's stdout alone. aid writes into the same
/// PTY, and the terminal echoes it back: an auto-nudge lands as a bare line
/// *before* grok's envelope, and the terminal sentinel lands after it. Requiring
/// the whole buffer to be one JSON document therefore failed every run that was
/// idle long enough to be nudged — measured on t-cd0bb8dd (8m16s, `end_turn`,
/// real work committed) and t-137fe385, both stored Failed, while a 27s run that
/// was never nudged parsed fine. The stopReason check below never ran in exactly
/// the cases it was written for.
pub fn parse_grok_completion(output: &str) -> CompletionInfo {
    let Some(value) = extract_envelope(output) else {
        return failed_completion();
    };
    if value.get("type").and_then(Value::as_str) == Some("error") {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            maybe_mark_rate_limit(message);
        }
        return failed_completion();
    }
    // grok reports a cut-short run in the same envelope shape as a good one: no
    // `type: "error"`, a populated `text`, real usage and real cost. Only
    // `stopReason` tells them apart, and without this check every truncated run
    // recorded as Done — `t-560628e5` and `t-2a1b09aa` are still stored that
    // way, and `t-c7ae82a8` stopped mid-sentence inside its own Findings
    // section after 5 turns and $0.22, having spent real money on a report
    // nobody could use.
    //
    // Keyed to values actually captured rather than to a guessed enum:
    // `end_turn` on completed runs, `cancelled` on truncated ones. An
    // unrecognised value is left alone instead of failed — inventing the shape
    // of output we have not seen is what got the previous round of completion
    // detectors blocked.
    if value.get("stopReason").and_then(Value::as_str) == Some("cancelled") {
        return failed_completion();
    }
    let tokens = value
        .pointer("/usage/total_tokens")
        .and_then(Value::as_i64)
        .filter(|total| *total > 0);
    let model = extract_model_usage_key(&value);
    let cost_usd = value
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .or_else(|| model_usage_cost(&value));
    CompletionInfo {
        tokens,
        status: TaskStatus::Done,
        model,
        cost_usd,
        exit_code: None,
    }
}

fn failed_completion() -> CompletionInfo {
    CompletionInfo {
        tokens: None,
        status: TaskStatus::Failed,
        model: None,
        cost_usd: None,
        exit_code: None,
    }
}

fn extract_model_usage_key(value: &Value) -> Option<String> {
    value
        .get("modelUsage")
        .and_then(Value::as_object)
        .and_then(|usage| usage.keys().next())
        .cloned()
}

fn model_usage_cost(value: &Value) -> Option<f64> {
    let usage = value.get("modelUsage")?.as_object()?;
    usage.values().find_map(|entry| entry.get("costUSD").and_then(Value::as_f64))
}

fn maybe_mark_rate_limit(detail: &str) {
    if rate_limit::is_rate_limit_error_for_agent(detail, &AgentKind::Grok) {
        rate_limit::mark_rate_limited(&AgentKind::Grok, None, detail);
    }
}

pub fn make_completion_event(task_id: &TaskId, info: &CompletionInfo) -> TaskEvent {
    let detail = match info.tokens {
        Some(tokens) => format!("completed with {tokens} tokens"),
        None => "completed".to_string(),
    };
    let mut metadata = json!({});
    let mut has_fields = false;
    if let Some(tokens) = info.tokens {
        metadata["tokens"] = json!(tokens);
        has_fields = true;
    }
    if let Some(model) = info.model.as_deref() {
        metadata["model"] = json!(model);
        has_fields = true;
    }
    if let Some(cost_usd) = info.cost_usd {
        metadata["cost_usd"] = json!(cost_usd);
        has_fields = true;
    }
    TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
        detail,
        metadata: has_fields.then_some(metadata),
    }
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod tests;

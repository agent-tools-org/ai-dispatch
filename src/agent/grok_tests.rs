use super::*;
use crate::agent::Agent;
use crate::types::{AgentKind, TaskId, TaskStatus};

fn opts(read_only: bool) -> RunOpts {
    RunOpts {
        dir: Some("/tmp".to_string()),
        output: None,
        result_file: None,
        model: Some("grok-4.5".to_string()),
        budget: false,
        read_only,
        sandbox: false,
        context_files: Vec::new(),
        session_id: Some("sess-1".to_string()),
        env: None,
        env_forward: None,
    }
}

fn args_for(opts: &RunOpts) -> Vec<String> {
    GrokAgent
        .build_command("hello", opts)
        .expect("build command")
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn build_command_uses_grok_binary_and_json_flags() {
    let args = args_for(&opts(false));
    assert_eq!(
        GrokAgent
            .build_command("hello", &opts(false))
            .expect("build")
            .get_program()
            .to_string_lossy(),
        "grok"
    );
    assert!(args.windows(2).any(|pair| pair == ["-p", "hello"]));
    assert!(args.windows(2).any(|pair| pair == ["--output-format", "json"]));
    assert!(args.windows(2).any(|pair| pair == ["--model", "grok-4.5"]));
    assert!(args.windows(2).any(|pair| pair == ["--cwd", "/tmp"]));
    assert!(args.windows(2).any(|pair| pair == ["-r", "sess-1"]));
}

/// Headless grok inherits Claude Code Stop hooks that call `hiboss ask` and
/// block forever without a human. The deny must ship on every dispatch.
#[test]
fn headless_dispatch_denies_hiboss_bash() {
    for read_only in [false, true] {
        let args = args_for(&opts(read_only));
        assert!(
            args.windows(2).any(|pair| pair == ["--deny", "Bash(hiboss:*)"]),
            "read_only={read_only}: missing hiboss deny"
        );
    }
}

#[test]
fn read_only_uses_plan_permission_mode() {
    let args = args_for(&opts(true));
    assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "plan"]));
    assert!(
        !args.iter().any(|arg| arg == "--always-approve"),
        "a read-only run must never carry blanket approval"
    );
}

/// A write run without this cancels its own tool calls and still bills: headless
/// grok renders no approval prompt, it abandons the call and exits 0 with
/// `stopReason: "cancelled"`.
#[test]
fn write_run_carries_blanket_approval() {
    let args = args_for(&opts(false));
    assert!(args.iter().any(|arg| arg == "--always-approve"));
    assert!(!args.iter().any(|arg| arg == "plan"));
}

#[test]
fn read_only_with_result_file_allows_write_for_report() {
    let mut run = opts(true);
    run.result_file = Some("result.md".to_string());
    let args = args_for(&run);
    assert!(args.iter().any(|arg| arg == "--always-approve"));
    assert!(!args.windows(2).any(|pair| pair == ["--permission-mode", "plan"]));
    let prompt = args
        .windows(2)
        .find(|pair| pair[0] == "-p")
        .map(|pair| pair[1].as_str())
        .unwrap();
    assert!(prompt.contains("EXCEPT the result file"));
}

#[test]
fn streaming_is_false_and_parse_event_returns_none() {
    let task_id = TaskId::generate();
    assert!(!GrokAgent.streaming());
    assert!(GrokAgent.parse_event(&task_id, "anything").is_none());
}

/// grok runs buffered: nothing reaches the PTY until exit. Without --debug-file
/// both reapers see zero bytes and kill healthy runs at 183s (t-73b69cde,
/// t-24f12f38). The adapter must pass --debug-file when env carries the log path.
#[test]
fn build_command_passes_debug_file_when_agent_log_env_is_set() {
    let mut run = opts(false);
    run.env = Some(std::collections::HashMap::from([(
        crate::agent::AGENT_LOG_ENV.to_string(),
        "/tmp/aid-test/tasks/t-grok1234/agent.log".to_string(),
    )]));
    let args = GrokAgent.build_command("task", &run).unwrap()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let idx = args.iter().position(|a| a == "--debug-file")
        .expect("--debug-file must be passed when AID_AGENT_LOG is set");
    let path = &args[idx + 1];
    assert!(path.contains("t-grok1234"), "debug-file must be per task: {path}");
    assert!(path.ends_with("agent.log"), "and named for aid's watcher: {path}");
}

/// Without a log path in env there is nothing to name the file after; passing a
/// bare --debug-file flag would be worse than passing none.
#[test]
fn build_command_omits_debug_file_when_no_agent_log_env() {
    let args = args_for(&opts(false));
    assert!(
        !args.iter().any(|a| a == "--debug-file"),
        "--debug-file must not appear when env carries no log path; got: {args:?}"
    );
}

#[test]
fn parse_completion_reads_model_tokens_and_cost_from_model_usage() {
    let output = r#"{
      "text": "OK",
      "usage": {
        "input_tokens": 100,
        "output_tokens": 5,
        "total_tokens": 105
      },
      "total_cost_usd": 0.42,
      "modelUsage": {
        "grok-4.5-build": {
          "inputTokens": 100,
          "outputTokens": 5,
          "costUSD": 0.42
        }
      }
    }"#;
    let completion = GrokAgent.parse_completion(output);
    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.tokens, Some(105));
    assert_eq!(completion.model.as_deref(), Some("grok-4.5-build"));
    assert_eq!(completion.cost_usd, Some(0.42));
}

#[test]
fn parse_completion_marks_unknown_model_errors_failed() {
    let output = r#"{"type":"error","message":"Couldn't set model 'x': Invalid params: \"unknown model id\"."}"#;
    let completion = GrokAgent.parse_completion(output);
    assert_eq!(completion.status, TaskStatus::Failed);
    assert!(completion.model.is_none());
}

#[test]
fn extract_response_returns_text_field() {
    let output = r#"{"text":"OK","usage":{"total_tokens":1}}"#;
    assert_eq!(extract_response(output).as_deref(), Some("OK"));
}

#[test]
fn kind_returns_grok() {
    assert_eq!(GrokAgent.kind(), AgentKind::Grok);
}

#[test]
fn catalog_lists_grok_profile_and_default_model() {
    use crate::model_catalog::{AGENT_MODELS, AGENT_PROFILES};
    let profile = AGENT_PROFILES
        .iter()
        .find(|(agent, _, _, _, _)| *agent == AgentKind::Grok);
    let Some((_, _, cost, _, streaming)) = profile else {
        panic!("grok missing from AGENT_PROFILES");
    };
    assert_eq!(*cost, "unknown");
    assert!(!*streaming);
    let model = AGENT_MODELS
        .iter()
        .find(|m| m.agent == AgentKind::Grok && m.model == "grok-4.5");
    let Some(model) = model else {
        panic!("grok-4.5 missing from AGENT_MODELS");
    };
    assert_eq!(model.tier, "unknown");
    assert_eq!(model.input_per_m, 0.0);
    assert_eq!(model.output_per_m, 0.0);
    // Catalog 0.0 must not become a numeric cost. Without agent-reported
    // total_cost_usd, estimate_cost stays None → format "unknown", never $0.00.
    //
    // Asked of the catalog alone: a real price feed on the developer's machine
    // does know grok's rate, and using it is correct. This test failed in the
    // release run for exactly that reason — it was asserting the feed away
    // rather than isolating it. Clearing the process cache is not enough:
    // feed_index() reloads from AID_HOME on the next miss, so the home is what
    // has to be isolated.
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::cost::clear_feed_for_tests();
    for model_name in ["grok-4.5", "grok-4.5-build"] {
        let cost = crate::cost::estimate_cost(100_000, Some(model_name), AgentKind::Grok);
        assert_eq!(cost, None, "{model_name}");
        assert_eq!(crate::cost::format_cost(cost), "unknown");
    }
    assert_eq!(
        crate::cost::estimate_cost(100_000, None, AgentKind::Grok),
        None
    );
}

/// Taken verbatim in shape from `t-c7ae82a8`: no `type: "error"`, a populated
/// `text`, real usage and real cost — the only thing separating it from a good
/// run is `stopReason`. It was stored as Done.
#[test]
fn parse_completion_marks_a_cancelled_run_failed() {
    let output = r#"{"text":"Findings. Severity: Critical. File: web/api.","stopReason":"cancelled","num_turns":5,"total_cost_usd":0.2196304,"modelUsage":{"grok-4.5-build":{"costUSD":0.2196304}}}"#;
    let completion = GrokAgent.parse_completion(output);
    assert_eq!(completion.status, TaskStatus::Failed);
}

/// The negative control the paired check needs: the same envelope with the
/// value real completed runs carry (`t-7aeb222c`, `t-bdbfa210`) must stay Done,
/// keeping its model and cost attribution.
#[test]
fn parse_completion_keeps_an_end_turn_run_done() {
    let output = r#"{"text":"done","stopReason":"end_turn","usage":{"total_tokens":249713},"total_cost_usd":0.1616412,"modelUsage":{"grok-4.5-build":{"costUSD":0.1616412}}}"#;
    let completion = GrokAgent.parse_completion(output);
    assert_eq!(completion.status, TaskStatus::Done);
    assert_eq!(completion.model.as_deref(), Some("grok-4.5-build"));
    assert_eq!(completion.tokens, Some(249713));
}

/// An envelope with no `stopReason` at all, and one carrying a value we have
/// never captured, must not be failed on a guess.
#[test]
fn parse_completion_does_not_fail_on_an_unseen_stop_reason() {
    for output in [
        r#"{"text":"ok","usage":{"total_tokens":5}}"#,
        r#"{"text":"ok","stopReason":"max_tokens","usage":{"total_tokens":5}}"#,
    ] {
        assert_eq!(GrokAgent.parse_completion(output).status, TaskStatus::Done);
    }
}

/// aid writes its auto-nudge into the same PTY grok is reading, and the terminal
/// echoes it back ahead of grok's envelope. Requiring the whole buffer to be one
/// JSON document failed every run idle long enough to be nudged: t-cd0bb8dd ran
/// 8m16s, reported `end_turn`, committed real work, and was stored Failed.
#[test]
fn completion_survives_an_echoed_nudge_before_the_envelope() {
    let output = concat!(
        "Task appears idle. Status update please?\n",
        r#"{"text":"done","stopReason":"end_turn","usage":{"total_tokens":42},"#,
        r#""total_cost_usd":0.75}"#,
    );
    let info = GrokAgent.parse_completion(output);
    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.tokens, Some(42));
    assert_eq!(extract_response(output).as_deref(), Some("done"));
}

/// The terminal sentinel aid appends after the run must not break it either.
#[test]
fn completion_survives_a_trailing_sentinel() {
    let output = concat!(
        "Task appears idle. Status update please?\n",
        r#"{"text":"done","stopReason":"cancelled"}"#,
        "\n\n=== AID TASK t-abc DONE (exit 0) ===\n",
    );
    // Still failed — but now for the real reason, not a parse error.
    assert_eq!(GrokAgent.parse_completion(output).status, TaskStatus::Failed);
}

/// The common case, and the one the first version of extract_envelope missed:
/// grok's envelope starts at offset 0 and aid's terminal sentinel follows it.
/// finalize_buffered appends that sentinel before parse_completion runs, so this
/// shape — not the nudged one — is what every grok run actually produces.
#[test]
fn completion_survives_a_sentinel_after_an_envelope_at_offset_zero() {
    let output = concat!(
        r#"{"text":"done","usage":{"total_tokens":7},"total_cost_usd":0.24}"#,
        "\n\n=== AID TASK t-abc DONE (exit 0) ===\n",
    );
    let info = GrokAgent.parse_completion(output);
    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.tokens, Some(7));
    assert_eq!(extract_response(output).as_deref(), Some("done"));
}

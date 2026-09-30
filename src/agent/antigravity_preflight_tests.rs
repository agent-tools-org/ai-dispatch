// Requested-model preflight and fallback command behavior for agy.
// Exercises the adapter validation contract with successful and failed help probes.
// Deps: AntigravityAgent, Agent, CliCommandOutput, RunOpts.

use super::AntigravityAgent;
use crate::agent::{Agent, CliCommandOutput, RunOpts};

#[test]
fn help_defining_model_on_either_stream_accepts_requested_model() {
    for (stdout, stderr) in [("  --model string\n", ""), ("", "  --model string\n")] {
        let runner = |_: &str, _: &[&str]| Ok(CliCommandOutput {
            success: true,
            stdout: stdout.into(),
            stderr: stderr.into(),
        });
        AntigravityAgent.validate_cli(Some("gemini-3-pro"), &runner).unwrap();
    }
}

#[test]
fn help_without_model_rejects_requested_model() {
    let runner = |_: &str, _: &[&str]| Ok(CliCommandOutput {
        success: true,
        stdout: "  --print\n  --add-dir string\n".into(),
        stderr: String::new(),
    });
    let error = AntigravityAgent.validate_cli(Some("gemini-3-pro"), &runner)
        .unwrap_err().to_string();
    assert!(error.contains("does not support --model"), "{error}");
    assert!(error.contains("gemini-3-pro"), "{error}");
    assert!(error.contains("Omit --model or upgrade agy"), "{error}");
}

#[test]
fn help_without_model_accepts_no_requested_model() {
    let runner = |_: &str, _: &[&str]| Ok(CliCommandOutput {
        success: true,
        stdout: "  --print\n".into(),
        stderr: String::new(),
    });
    AntigravityAgent.validate_cli(None, &runner).unwrap();
}

#[test]
fn timed_out_help_soft_skips_requested_model_check() {
    let runner = |_: &str, _: &[&str]| anyhow::bail!("agy --help timed out after 10s");
    AntigravityAgent.validate_cli(Some("gemini-3-pro"), &runner).unwrap();
}

#[test]
fn unsuccessful_help_soft_skips_requested_model_check() {
    let runner = |_: &str, _: &[&str]| Ok(CliCommandOutput {
        success: false,
        stdout: "  --print\n".into(),
        stderr: "help unavailable".into(),
    });
    AntigravityAgent.validate_cli(Some("gemini-3-pro"), &runner).unwrap();
}

#[test]
fn fallback_command_drops_model_when_flags_are_unavailable() {
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: Some("gemini-3-pro".into()),
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: Vec::new(),
        session_id: None,
        env: None,
        env_forward: None,
    };
    let command = AntigravityAgent.build_command("Inspect the module", &opts).unwrap();
    let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(!args.iter().any(|arg| arg == "--model" || arg == "-m" || arg == "gemini-3-pro"));
    assert!(args.windows(2).any(|pair| pair == ["-p", "Inspect the module"]));
}

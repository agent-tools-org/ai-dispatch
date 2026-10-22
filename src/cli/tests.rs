// aid CLI parser tests.
// Covers top-level command parsing; depends on clap Parser and cli module exports.

use super::{Cli, Commands, run_args};
use clap::Parser;

#[test]
fn bare_aid_parses_without_subcommand() {
    let cli = Cli::try_parse_from(["aid"]).unwrap();
    assert!(cli.command.is_none());
}

#[test]
fn run_best_of_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "auto", "add tests", "--best-of", "3"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { best_of, .. })) => {
            assert_eq!(best_of, Some(3))
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn run_parent_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "do stuff", "--parent", "t-abc123"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { parent, .. })) => {
            assert_eq!(parent, Some("t-abc123".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_peer_review_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--peer-review", "gemini"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { peer_review, .. })) => {
            assert_eq!(peer_review, Some("gemini".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_timeout_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--timeout", "300"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { timeout, .. })) => assert_eq!(timeout, Some(300)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_audit_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--audit"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { audit, .. })) => assert!(audit),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_idle_timeout_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--idle-timeout", "240"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { idle_timeout, .. })) => {
            assert_eq!(idle_timeout, Some(240))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_no_link_deps_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--no-link-deps"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { no_link_deps, .. })) => {
            assert!(no_link_deps)
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_sandbox_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--sandbox"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { sandbox, .. })) => assert!(sandbox),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_container_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--container", "dev:latest"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { container, .. })) => {
            assert_eq!(container, Some("dev:latest".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_result_file_flag_parses() {
    let cli =
        Cli::try_parse_from(["aid", "run", "codex", "task", "--result-file", "/tmp/result.md"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs { result_file, .. })) => {
            assert_eq!(result_file, Some("/tmp/result.md".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_prompt_file_flag_parses_without_positional_prompt() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "--prompt-file", "/tmp/prompt.md"]).unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs {
            prompt, prompt_file, ..
        })) => {
            assert!(prompt.is_none());
            assert_eq!(prompt_file, Some("/tmp/prompt.md".to_string()));
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_iterate_flags_parse() {
    let cli = Cli::try_parse_from([
        "aid",
        "run",
        "codex",
        "task",
        "--iterate",
        "3",
        "--eval",
        "echo ok",
        "--eval-feedback-template",
        "Iteration {iteration}/{max_iterations}: {eval_output}",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Run(run_args::RunArgs {
            iterate,
            eval,
            eval_feedback_template,
            ..
        })) => {
            assert_eq!(iterate, Some(3));
            assert_eq!(eval, Some("echo ok".to_string()));
            assert_eq!(
                eval_feedback_template,
                Some("Iteration {iteration}/{max_iterations}: {eval_output}".to_string())
            );
        }
        _ => panic!("expected Run"),
    }
}

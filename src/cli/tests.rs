// aid CLI parser tests.
// Covers top-level command parsing; depends on clap Parser and cli module exports.

use super::{BatchAction, Cli, Commands, ExperimentCommands, HookAction};
use super::{command_args_a, command_args_b};
use crate::cli_actions::ContainerAction;
use clap::Parser;

#[test]
fn run_best_of_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "auto", "add tests", "--best-of", "3"]).unwrap();
    match cli.command {
        Commands::Run(command_args_a::RunArgs { best_of, .. }) => assert_eq!(best_of, Some(3)),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn run_parent_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "do stuff", "--parent", "t-abc123"]).unwrap();
    match cli.command {
        Commands::Run(command_args_a::RunArgs { parent, .. }) => {
            assert_eq!(parent, Some("t-abc123".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_peer_review_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--peer-review", "gemini"]).unwrap();
    match cli.command {
        Commands::Run(command_args_a::RunArgs { peer_review, .. }) => {
            assert_eq!(peer_review, Some("gemini".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_timeout_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--timeout", "300"]).unwrap();
    match cli.command {
        Commands::Run(command_args_a::RunArgs { timeout, .. }) => assert_eq!(timeout, Some(300)),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_sandbox_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--sandbox"]).unwrap();
    match cli.command {
        Commands::Run(command_args_a::RunArgs { sandbox, .. }) => assert!(sandbox),
        _ => panic!("expected Run"),
    }
}

#[test]
fn run_container_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "run", "codex", "task", "--container", "dev:latest"]).unwrap();
    match cli.command {
        Commands::Run(command_args_a::RunArgs { container, .. }) => {
            assert_eq!(container, Some("dev:latest".to_string()))
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn watch_timeout_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "watch", "--quiet", "--timeout", "60", "--group", "wg-a"]).unwrap();
    match cli.command {
        Commands::Watch(command_args_a::WatchArgs { timeout, group, quiet, .. }) => {
            assert!(quiet);
            assert_eq!(timeout, Some(60));
            assert_eq!(group, Some("wg-a".to_string()));
        }
        _ => panic!("expected Watch"),
    }
}

#[test]
fn experiment_run_parses() {
    let cli = Cli::try_parse_from([
        "aid",
        "experiment",
        "run",
        "codex",
        "optimize perf",
        "--metric",
        "cargo bench 2>&1 | tail -1",
        "--direction",
        "min",
        "--max-runs",
        "10",
    ])
    .unwrap();
    match cli.command {
        Commands::Experiment(ExperimentCommands::Run { agent, max_runs, .. }) => {
            assert_eq!(agent, "codex");
            assert_eq!(max_runs, 10);
        }
        _ => panic!("expected Experiment Run"),
    }
}

#[test]
fn hook_session_start_parses() {
    let cli = Cli::try_parse_from(["aid", "hook", "session-start"]).unwrap();
    match cli.command {
        Commands::Hook(command_args_b::HookArgs { action: HookAction::SessionStart }) => {}
        _ => panic!("expected Hook SessionStart"),
    }
}

#[test]
fn container_subcommand_parses() {
    let cli = Cli::try_parse_from(["aid", "container", "stop", "aid-dev-demo"]).unwrap();
    match cli.command {
        Commands::Container(command_args_b::ContainerArgs {
            action: ContainerAction::Stop { name },
        }) => assert_eq!(name, "aid-dev-demo"),
        _ => panic!("expected Container stop"),
    }
}

#[test]
fn batch_dispatch_file_parses() {
    let cli = Cli::try_parse_from(["aid", "batch", "tasks.toml", "--parallel", "--var", "project=demo"]).unwrap();
    match cli.command {
        Commands::Batch(command_args_a::BatchArgs { action, file, vars, parallel, .. }) => {
            assert!(action.is_none());
            assert_eq!(file, Some("tasks.toml".to_string()));
            assert_eq!(vars, vec!["project=demo".to_string()]);
            assert!(parallel);
        }
        _ => panic!("expected Batch"),
    }
}

#[test]
fn batch_retry_parses() {
    let cli = Cli::try_parse_from(["aid", "batch", "retry", "wg-a", "--agent", "cursor"]).unwrap();
    match cli.command {
        Commands::Batch(command_args_a::BatchArgs {
            action: Some(BatchAction::Retry { group_id, agent }),
            file,
            vars,
            ..
        }) => {
            assert_eq!(group_id, "wg-a");
            assert_eq!(agent, Some("cursor".to_string()));
            assert!(file.is_none());
            assert!(vars.is_empty());
        }
        _ => panic!("expected Batch retry"),
    }
}

#[test]
fn changelog_version_parses() {
    let cli = Cli::try_parse_from(["aid", "changelog", "--version", "8.21.14"]).unwrap();
    match cli.command {
        Commands::Changelog(command_args_a::ChangelogArgs { version, all, count, git }) => {
            assert_eq!(version, Some("8.21.14".to_string()));
            assert!(!all);
            assert_eq!(count, 5);
            assert!(!git);
        }
        _ => panic!("expected Changelog"),
    }
}

#[test]
fn changelog_git_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "changelog", "--git"]).unwrap();
    match cli.command {
        Commands::Changelog(command_args_a::ChangelogArgs { git, .. }) => assert!(git),
        _ => panic!("expected Changelog"),
    }
}

#[test]
fn show_summary_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--summary"]).unwrap();
    match cli.command {
        Commands::Show(command_args_a::ShowArgs { task_id, summary, diff, file, .. }) => {
            assert_eq!(task_id, "t-1234");
            assert!(summary);
            assert!(!diff);
            assert_eq!(file, None);
        }
        _ => panic!("expected Show"),
    }
}

#[test]
fn show_diff_file_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--diff", "--file", "src/cli.rs"]).unwrap();
    match cli.command {
        Commands::Show(command_args_a::ShowArgs { diff, summary, file, .. }) => {
            assert!(diff);
            assert!(!summary);
            assert_eq!(file, Some("src/cli.rs".to_string()));
        }
        _ => panic!("expected Show"),
    }
}

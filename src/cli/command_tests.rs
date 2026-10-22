// aid CLI parser tests.
// Covers top-level command parsing; depends on clap Parser and cli module exports.

use super::{BatchAction, Cli, Commands, ExperimentCommands, HookAction};
use super::{admin_args, group_args, inspect_args, run_args};
use crate::cli::{ContainerAction, GroupAction};
use clap::Parser;

#[test]
fn watch_timeout_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "watch", "--timeout", "60", "--group", "wg-a"]).unwrap();
    match cli.command {
        Some(Commands::Watch(inspect_args::WatchArgs {
            timeout,
            group,
            stream,
            ..
        })) => {
            assert!(!stream);
            assert_eq!(timeout, Some(60));
            assert_eq!(group, Some("wg-a".to_string()));
        }
        _ => panic!("expected Watch"),
    }
}

#[test]
fn watch_stream_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "watch", "--stream", "--group", "wg-a"]).unwrap();
    match cli.command {
        Some(Commands::Watch(inspect_args::WatchArgs { group, stream, .. })) => {
            assert!(stream);
            assert_eq!(group, Some("wg-a".to_string()));
        }
        _ => panic!("expected Watch"),
    }
}

#[test]
fn watch_stream_conflicts_with_tui() {
    let err = match Cli::try_parse_from(["aid", "watch", "--stream", "--tui", "t-1234"]) {
        Ok(_) => panic!("expected stream/tui conflict"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("cannot be used with"));
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
        Some(Commands::Experiment(ExperimentCommands::Run { agent, max_runs, .. })) => {
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
        Some(Commands::Hook(admin_args::HookArgs {
            action: HookAction::SessionStart,
        })) => {}
        _ => panic!("expected Hook SessionStart"),
    }
}

#[test]
fn container_subcommand_parses() {
    let cli = Cli::try_parse_from(["aid", "container", "stop", "aid-dev-demo"]).unwrap();
    match cli.command {
        Some(Commands::Container(admin_args::ContainerArgs {
            action: ContainerAction::Stop { name },
        })) => assert_eq!(name, "aid-dev-demo"),
        _ => panic!("expected Container stop"),
    }
}

#[test]
fn group_cancel_subcommand_parses() {
    let cli = Cli::try_parse_from(["aid", "group", "cancel", "wg-a"]).unwrap();
    match cli.command {
        Some(Commands::Group(group_args::GroupArgs {
            action: GroupAction::Cancel { group_id },
        })) => assert_eq!(group_id, "wg-a"),
        _ => panic!("expected Group cancel"),
    }
}

#[test]
fn batch_dispatch_file_parses() {
    let cli = Cli::try_parse_from([
        "aid",
        "batch",
        "tasks.toml",
        "--parallel",
        "--analyze",
        "--force",
        "--var",
        "project=demo",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Batch(run_args::BatchArgs {
            action,
            file,
            vars,
            parallel,
            analyze,
            force,
            ..
        })) => {
            assert!(action.is_none());
            assert_eq!(file, Some("tasks.toml".to_string()));
            assert_eq!(vars, vec!["project=demo".to_string()]);
            assert!(parallel);
            assert!(analyze);
            assert!(force);
        }
        _ => panic!("expected Batch"),
    }
}

#[test]
fn batch_retry_parses() {
    let cli = Cli::try_parse_from([
        "aid",
        "batch",
        "retry",
        "wg-a",
        "--agent",
        "cursor",
        "--include-waiting",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Batch(run_args::BatchArgs {
            action:
                Some(BatchAction::Retry {
                    group_id,
                    agent,
                    include_waiting,
                }),
            file,
            vars,
            ..
        })) => {
            assert_eq!(group_id, "wg-a");
            assert_eq!(agent, Some("cursor".to_string()));
            assert!(include_waiting);
            assert!(file.is_none());
            assert!(vars.is_empty());
        }
        _ => panic!("expected Batch retry"),
    }
}

#[test]
fn batch_no_prompt_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "batch", "tasks.toml", "--no-prompt"]).unwrap();
    match cli.command {
        Some(Commands::Batch(run_args::BatchArgs { no_prompt, yes, .. })) => {
            assert!(no_prompt);
            assert!(!yes);
        }
        _ => panic!("expected Batch"),
    }
}

#[test]
fn batch_yes_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "batch", "tasks.toml", "--yes"]).unwrap();
    match cli.command {
        Some(Commands::Batch(run_args::BatchArgs { yes, no_prompt, .. })) => {
            assert!(yes);
            assert!(!no_prompt);
        }
        _ => panic!("expected Batch"),
    }
}

#[test]
fn changelog_version_parses() {
    let cli = Cli::try_parse_from(["aid", "changelog", "--version", "8.21.14"]).unwrap();
    match cli.command {
        Some(Commands::Changelog(inspect_args::ChangelogArgs {
            version,
            all,
            count,
            git,
        })) => {
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
        Some(Commands::Changelog(inspect_args::ChangelogArgs { git, .. })) => assert!(git),
        _ => panic!("expected Changelog"),
    }
}

#[test]
fn show_summary_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--summary"]).unwrap();
    match cli.command {
        Some(Commands::Show(inspect_args::ShowArgs {
            task_id,
            summary,
            diff,
            file,
            ..
        })) => {
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
        Some(Commands::Show(inspect_args::ShowArgs {
            diff, summary, file, ..
        })) => {
            assert!(diff);
            assert!(!summary);
            assert_eq!(file, Some("src/cli.rs".to_string()));
        }
        _ => panic!("expected Show"),
    }
}

#[test]
fn show_result_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--result"]).unwrap();
    match cli.command {
        Some(Commands::Show(inspect_args::ShowArgs { result, .. })) => assert!(result),
        _ => panic!("expected Show"),
    }
}

#[test]
fn show_transcript_flag_parses() {
    let cli = Cli::try_parse_from(["aid", "show", "t-1234", "--transcript"]).unwrap();
    match cli.command {
        Some(Commands::Show(inspect_args::ShowArgs {
            task_id, transcript, ..
        })) => {
            assert_eq!(task_id, "t-1234");
            assert!(transcript);
        }
        _ => panic!("expected Show"),
    }
}

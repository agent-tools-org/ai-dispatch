// aid — Multi-AI CLI team orchestrator.
// Entry point wiring: modules, CLI parsing, and dispatch startup.

#![allow(dead_code)]
#![allow(clippy::boxed_local)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::filter_map_bool_then)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_unwrap_or)]
#![allow(clippy::needless_return)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_map_or)]

#[macro_use]
pub mod output;
mod agent;
mod agent_config;
mod aic;
mod background;
mod batch;
mod board;
mod cli_actions;
mod cmd;
mod cmd_dispatch;
mod commit;
mod config;
mod container;
mod context;
mod cost;
pub mod credential_pool;
mod explore;
mod failure_salvage;
mod delivery_guard;
pub mod gitbutler;
mod hooks;
mod idle_timeout;
mod input_signal;
mod model_catalog;
mod model_health;
mod notify;
mod paths;
mod prompt;
mod prompt_scan;
pub mod process_group;
mod process_guard;
mod process_monitor;
mod pty_bridge;
mod pty_runner;
mod pty_runner_control;
mod pty_watch_idle;
mod pty_watch;
mod rate_limit;
mod rate_limit_signatures;
mod rate_limit_wait;
mod repo_root;
pub(crate) mod sanitize;
mod sandbox;
#[cfg(test)]
mod sandbox_tests;
mod session;
mod shared_dir;
mod skills;
mod state;
#[cfg(test)]
mod state_tests;
mod store;
mod store_workgroups;
mod artifact_custody;
mod task_actions;
mod task_lifecycle;
mod task_view;
mod project;
mod team;
mod templates;
mod timeout_policy;
mod toolbox;
mod unstick;
#[cfg(test)]
mod test_subprocess;
#[cfg(test)]
mod test_env;
mod compaction;
pub mod claudemd;
mod tui;
mod system_resources;
mod types;
mod update_check;
mod usage;
pub mod usage_report;
mod verify;
mod verify_declared_files;
mod watcher;
mod webhook;
#[cfg(feature = "web")]
mod web;
mod workgroup;
mod worktree_gc;
mod worktree_deps;
mod worktree;
mod worktree_layout;
mod cli;

use crate::cli::{Cli, Commands};
use anyhow::Result;
use clap::Parser;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    output::init();
    if cli.quiet {
        output::set_quiet(true);
    }

    let command = match cli.command {
        Some(Commands::Advise(args)) => {
            let store = store::Store::open_read_only(&paths::db_path())?;
            cmd::advise::run(store.as_ref(), args)?;
            return Ok(());
        }
        other => other,
    };

    paths::ensure_dirs()?;
    let config = config::load_config().unwrap_or_default();
    if config.updates.check {
        update_check::maybe_check_update();
    }
    // Refresh the model price feed out of band: never on the dispatch path, so
    // a cold or stale cache cannot delay or fail a run.
    cost::maybe_refresh_prices();
    let store = Arc::new(store::Store::open(&paths::db_path())?);
    cost::warm_gemini_default_from_store(store.as_ref());
    let _ = background::check_zombie_tasks(&store);

    let outcome = match command {
        Some(command) => cmd_dispatch::dispatch(store.clone(), normalize_command(command, cli.quiet)).await?,
        None => cmd_dispatch::dispatch(store.clone(), Commands::Board(Default::default())).await?,
    };
    if let Some(run_status) = outcome.run_exit_status(store.as_ref())? {
        println!("{}", run_status.summary_line());
        if run_status.exit_code() != 0 {
            std::process::exit(run_status.exit_code());
        }
    }
    Ok(())
}

fn normalize_command(command: Commands, quiet: bool) -> Commands {
    if !quiet {
        return command;
    }
    match command {
        Commands::Watch(mut args) => {
            args.wait = true;
            Commands::Watch(args)
        }
        other => other,
    }
}

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
mod batch;
mod board;
mod command_diagnostics;
mod cmd;
mod cmd_dispatch;
mod config;
mod cost;
mod prompt;
mod store;
mod tui;
mod types;
mod typesafe;
#[cfg(feature = "web")]
mod web;
mod cli;
mod support;
mod runtime;
mod routing;
mod task;
mod workspace;

use crate::cli::{Cli, Commands};
use anyhow::Result;
use std::sync::Arc;

fn main() -> Result<()> {
    let result = run_cli(command_diagnostics::parse());
    if let Err(error) = &result {
        command_diagnostics::record_error(error);
    }
    result
}

fn run_cli(cli: Cli) -> Result<()> {
    if let Some(Commands::Errors(args)) = cli.command.as_ref() {
        return command_diagnostics::show(args);
    }
    if matches!(cli.command.as_ref(), Some(Commands::InternalRunTask(_)))
        && runtime::background::daemonize_worker_if_requested()?
    {
        return Ok(());
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> Result<()> {
    output::init();
    if cli.quiet {
        output::set_quiet(true);
    }

    let command = match cli.command {
        Some(Commands::Advise(args)) => {
            // Advise reads the score feed, so it starts the detached refresh too, but only
            // in an initialised aid home: advise must not create state in a fresh one.
            if support::paths::db_path().exists() {
                cost::maybe_refresh_prices();
            }
            let store = store::Store::open_read_only(&support::paths::db_path())?;
            cmd::advise::run(store.as_ref(), args)?;
            return Ok(());
        }
        // Needs no store, dirs, or update check; exit codes are part of its contract.
        Some(Commands::Classify(args)) => std::process::exit(cmd::classify::run(args)),
        other => other,
    };

    support::paths::ensure_dirs()?;
    let config = config::load_config().unwrap_or_default();
    if config.updates.check {
        support::update_check::maybe_check_update();
    }
    // Refresh the model price feed out of band: never on the dispatch path, so
    // a cold or stale cache cannot delay or fail a run.
    cost::maybe_refresh_prices();
    let store = Arc::new(store::Store::open(&support::paths::db_path())?);
    cost::warm_gemini_default_from_store(store.as_ref());
    let _ = runtime::background::check_zombie_tasks(&store);

    let outcome = match command {
        Some(command) => cmd_dispatch::dispatch(store.clone(), command).await?,
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

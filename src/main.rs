// aid — Multi-AI CLI team orchestrator.
// Entry point wiring: modules, CLI parsing, and dispatch startup.

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
pub mod ui;
mod agent;
mod execution;
mod batch;
mod cmd;
mod config;
mod cost;
mod prompt;
mod store;
mod types;
mod typesafe;
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
    let result = run_cli(cli::diagnostics::parse());
    if let Err(error) = &result {
        if let Some(refusal) = error.downcast_ref::<execution::Refusal>() {
            println!("{}", refusal.body());
            std::process::exit(refusal.exit);
        }
        cli::diagnostics::record_error(error);
    }
    result
}

fn run_cli(cli: Cli) -> Result<()> {
    if let Some(Commands::Errors(args)) = cli.command.as_ref() {
        return cli::diagnostics::show(args);
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
    ui::output::init();
    if cli.quiet {
        ui::output::set_quiet(true);
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
        Some(Commands::Capabilities { .. }) => {
            println!("{}", execution::capabilities());
            return Ok(());
        }
        // Needs no store, dirs, or update check; exit codes are part of its contract.
        Some(Commands::Classify(args)) => std::process::exit(cmd::classify::run(args)),
        other => other,
    };

    if let Some(Commands::Run(args)) = command.as_ref() {
        execution::validate_cli(args)?;
        if args.json {
            ui::output::set_quiet(true);
        }
    }
    let store = command_store()?;

    let outcome = match command {
        Some(command) => cli::dispatch::dispatch(store.clone(), command).await?,
        None => cli::dispatch::dispatch(store.clone(), Commands::Board(Default::default())).await?,
    };
    if let Some(run_status) = outcome.run_exit_status(store.as_ref())? {
        println!("{}", run_status.summary_line());
        if run_status.exit_code() != 0 {
            std::process::exit(run_status.exit_code());
        }
    }
    Ok(())
}

fn command_store() -> Result<Arc<store::Store>> {
    support::paths::ensure_dirs()?;
    let config = config::load_config().unwrap_or_default();
    if config.updates.check {
        support::update_check::maybe_check_update();
    }
    // Refresh the model price feed out of band: never on the dispatch path, so
    // a cold or stale cache cannot delay or fail a run.
    cost::maybe_refresh_prices();
    let store = Arc::new(store::Store::open(&support::paths::db_path())?);
    let _ = runtime::background::check_zombie_tasks(&store);
    Ok(store)
}

#[cfg(test)]
#[path = "main_observation_tests.rs"]
mod observation_tests;

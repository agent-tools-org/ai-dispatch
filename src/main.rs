// aid — Multi-AI CLI team orchestrator.
// Dispatches tasks to gemini/codex/opencode, watches progress, audits results.

mod agent;
mod background;
mod batch;
mod board;
mod cli_actions;
mod cmd;
mod commit;
mod config;
mod context;
mod cost;
mod explore;
mod input_signal;
mod paths;
mod prompt;
mod pty_bridge;
mod pty_runner;
mod pty_watch;
mod session;
mod skills;
mod store;
mod store_workgroups;
mod templates;
mod tui;
mod types;
mod usage;
mod verify;
mod watcher;
mod webhook;
mod workgroup;
mod worktree;
use crate::cli_actions::{ConfigAction, GroupAction};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "aid", version, about = "Multi-AI CLI team orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Dispatch a task to an AI agent
    Run {
        /// Agent to use (auto, gemini, codex, opencode, cursor)
        agent: String,
        /// Prompt / task description
        prompt: String,
        /// Working directory for the agent
        #[arg(short, long)]
        dir: Option<String>,
        /// Output file path (for research tasks)
        #[arg(short, long)]
        output: Option<String>,
        /// Model override (for opencode)
        #[arg(short, long)]
        model: Option<String>,
        /// Run in a git worktree branch
        #[arg(short, long)]
        worktree: Option<String>,
        /// Reuse shared context from a workgroup
        #[arg(long)]
        group: Option<String>,
        /// Verify command (or auto-detect if flag given without value)
        #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
        verify: Option<String>,
        /// Max retry attempts on failure
        #[arg(long, default_value = "0")]
        retry: u32,
        /// Context files to inject
        #[arg(long)]
        context: Vec<String>,
        /// Methodology skills to inject
        #[arg(long)]
        skill: Vec<String>,
        /// Disable automatic skill injection
        #[arg(long, conflicts_with = "skill")]
        no_skill: bool,
        /// Run in background
        #[arg(long)]
        bg: bool,
        /// Command to run on task completion
        #[arg(long)]
        on_done: Option<String>,
    },
    /// Dispatch tasks from a TOML batch file
    Batch {
        /// Path to the batch TOML file
        file: String,
        /// Dispatch tasks in parallel
        #[arg(long)]
        parallel: bool,
        /// Wait for dispatched tasks to finish
        #[arg(long)]
        wait: bool,
    },
    /// Live progress / blocking wait (--quiet)
    Watch {
        /// Watch a specific task ID
        task_id: Option<String>,
        /// Restrict to one workgroup in multi-task mode
        #[arg(long)]
        group: Option<String>,
        /// Interactive TUI mode
        #[arg(long)]
        tui: bool,
        /// Silent blocking wait (replaces `aid wait`)
        #[arg(long)]
        quiet: bool,
    },
    /// List all tasks with status
    Board {
        /// Show only running tasks
        #[arg(long)]
        running: bool,
        /// Show only today's tasks
        #[arg(long)]
        today: bool,
        /// Show only tasks from the current caller session
        #[arg(long)]
        mine: bool,
        /// Show only tasks for one workgroup
        #[arg(long)]
        group: Option<String>,
    },
    /// Inspect task artifacts (events, diff, output, explain)
    Show {
        /// Task ID to inspect
        task_id: String,
        /// Show full worktree diff
        #[arg(long)]
        diff: bool,
        /// Print output file
        #[arg(long)]
        output: bool,
        /// Dispatch AI explanation (creates child task)
        #[arg(long)]
        explain: bool,
        /// Print raw log file
        #[arg(long)]
        log: bool,
        /// Agent for --explain (default: gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Model override for --explain
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Show task-history usage and configured cost budgets
    Usage {
        /// Filter to current caller session
        #[arg(long)]
        session: bool,
    },
    /// Retry a failed task with feedback
    Retry {
        task_id: String,
        #[arg(short, long)]
        feedback: String,
    },
    /// Send interactive input to a background task
    Respond {
        task_id: String,
        input: String,
    },
    /// Research/explore via cheap AI CLIs
    Ask {
        prompt: String,
        #[arg(long)]
        agent: Option<String>,
        #[arg(short, long)]
        model: Option<String>,
        #[arg(long)]
        files: Vec<String>,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Start MCP server (stdio)
    Mcp,
    /// Manage agent configuration and detection
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage shared-context workgroups
    Group {
        #[command(subcommand)]
        action: GroupAction,
    },
    #[command(hide = true, name = "__run-task")]
    InternalRunTask { task_id: String },
/// Print task output (shortcut for `show --output`)
    Output {
        /// Task ID
        task_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    paths::ensure_dirs()?;
    let store = Arc::new(store::Store::open(&paths::db_path())?);

    match cli.command {
        Commands::Run {
            agent,
            prompt,
            dir,
            output,
            model,
            worktree,
            group,
            verify,
            retry,
            context,
            skill,
            no_skill,
            bg,
            on_done,
        } => {
            let agent_name = if agent == "auto" {
                let selection_opts = agent::RunOpts {
                    dir: dir
                        .clone()
                        .or_else(|| worktree.as_ref().map(|_| ".".to_string())),
                    output: output.clone(),
                    model: model.clone(),
                };
                let (selected, reason) = agent::select_agent_with_reason(&prompt, &selection_opts);
                eprintln!("[aid] Auto-selected agent: {selected} (reason: {reason})");
                selected.as_str().to_string()
            } else {
                agent
            };
            let skills = if no_skill {
                vec![cmd::run::NO_SKILL_SENTINEL.to_string()]
            } else {
                skill
            };
            let _ = cmd::run::run(
                store,
                cmd::run::RunArgs {
                    agent_name,
                    prompt,
                    dir,
                    output,
                    model,
                    worktree,
                    base_branch: None,
                    group,
                    verify,
                    max_duration_mins: None,
                    retry,
                    context,
                    skills,
                    background: bg,
                    announce: true,
                    parent_task_id: None,
                    on_done,
                },
            )
            .await?;
        }
        Commands::Batch {
            file,
            parallel,
            wait,
        } => {
            cmd::batch::run(
                store,
                cmd::batch::BatchArgs {
                    file,
                    parallel,
                    wait,
                },
            )
            .await?;
        }
        Commands::Watch {
            task_id,
            group,
            tui: true,
            ..
        } => {
            tui::run(&store, tui::RunOptions { task_id, group })?;
        }
        Commands::Watch {
            task_id,
            group,
            tui: false,
            quiet,
        } => {
            cmd::watch::run(&store, task_id.as_deref(), group.as_deref(), quiet).await?;
        }
        Commands::Board {
            running,
            today,
            mine,
            group,
        } => {
            cmd::board::run(&store, running, today, mine, group.as_deref())?;
        }
        Commands::Show {
            task_id,
            diff,
            output,
            explain,
            log,
            agent,
            model,
        } => {
            cmd::show::run(
                store,
                cmd::show::ShowArgs {
                    task_id,
                    diff,
                    output,
                    explain,
                    log,
                    agent,
                    model,
                },
            )
            .await?;
        }
        Commands::Output { task_id } => {
            let store = store::Store::open(&paths::db_path())?;
            let text = cmd::show::output_text_for_task(&store, &task_id)?;
            print!("{text}");
        }
        Commands::Usage { session } => {
            cmd::usage::run(&store, session)?;
        }
        Commands::Retry { task_id, feedback } => {
            cmd::retry::run(store, cmd::retry::RetryArgs { task_id, feedback }).await?;
        }
        Commands::Respond { task_id, input } => {
            cmd::respond::run(&task_id, &input)?;
        }
        Commands::Ask {
            prompt,
            agent,
            model,
            files,
            output,
        } => {
            cmd::ask::run(store, prompt, agent, model, files, output).await?;
        }
        Commands::Mcp => cmd::mcp::run(store).await?,
        Commands::Config { action } => {
            cmd::config::run(&store, action)?;
        }
        Commands::Group { action } => match action {
            GroupAction::Create { name, context } => cmd::group::create(&store, &name, &context)?,
            GroupAction::List => cmd::group::list(&store)?,
            GroupAction::Show { group_id } => cmd::group::show(&store, &group_id)?,
            GroupAction::Update {
                group_id,
                name,
                context,
            } => cmd::group::update(&store, &group_id, name.as_deref(), context.as_deref())?,
            GroupAction::Delete { group_id } => cmd::group::delete(&store, &group_id)?,
        },
        Commands::InternalRunTask { task_id } => {
            background::run_task(store, &task_id).await?;
        }
    }

    Ok(())
}

// aid — Multi-AI CLI team orchestrator.
// Dispatches tasks to gemini/codex/opencode, watches progress, audits results.

mod agent;
mod background;
mod batch;
mod board;
mod cmd;
mod config;
mod context;
mod cost;
mod explore;
mod paths;
mod select;
mod session;
mod store;
mod store_workgroups;
mod templates;
mod tui;
mod types;
mod usage;
mod verify;
mod watcher;
mod workgroup;
mod worktree;

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
        /// Run in background
        #[arg(long)]
        bg: bool,
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
    /// Live progress dashboard
    Watch {
        /// Watch a specific task ID
        task_id: Option<String>,
        /// Restrict to one workgroup in multi-task mode
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        tui: bool,
    },
    /// Block until a task or the current running set finishes
    Wait {
        task_id: Option<String>,
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
    /// Show detailed task audit
    Audit {
        /// Task ID to audit
        task_id: String,
    },
    /// Print the output file for a task
    Output {
        task_id: String,
    },
    /// Review worktree diff and events for a task
    Review {
        task_id: String,
    },
    /// Show task-history usage and configured cost budgets
    Usage,
    /// Retry a failed task with feedback
    Retry {
        task_id: String,
        #[arg(short, long)]
        feedback: String,
    },
    /// Explore codebase via cheap AI CLIs
    Explore {
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
    /// Manage agent configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage shared-context workgroups
    Group {
        #[command(subcommand)]
        action: GroupAction,
    },
    /// Show detected agents
    Agents,
    #[command(hide = true, name = "__run-task")]
    InternalRunTask {
        task_id: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// List configured agents
    Agents,
    /// Register custom agent
    AddAgent {
        name: String,
        command: String,
        #[arg(long)]
        streaming: bool,
    },
    /// Show pricing table
    Pricing,
}

#[derive(Subcommand)]
enum GroupAction {
    /// Create a workgroup with shared context
    Create {
        name: String,
        #[arg(long)]
        context: String,
    },
    /// List workgroups
    List,
    /// Show one workgroup and its member tasks
    Show {
        group_id: String,
    },
    /// Update a workgroup name and/or shared context
    Update {
        group_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        context: Option<String>,
    },
    /// Delete a workgroup definition
    Delete {
        group_id: String,
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
            bg,
        } => {
            let agent_name = if agent == "auto" {
                let selected = select::select_agent(&prompt, worktree.is_some());
                println!("Selected agent: {}", selected);
                selected.as_str().to_string()
            } else {
                agent
            };
            let _ = cmd::run::run(store, cmd::run::RunArgs {
                agent_name,
                prompt,
                dir,
                output,
                model,
                worktree,
                group,
                verify,
                retry,
                context,
                background: bg,
                parent_task_id: None,
            }).await?;
        }
        Commands::Batch {
            file,
            parallel,
            wait,
        } => {
            cmd::batch::run(store, cmd::batch::BatchArgs { file, parallel, wait }).await?;
        }
        Commands::Watch { task_id, group, tui } => {
            if tui {
                tui::run(&store)?;
            } else {
                cmd::watch::run(&store, task_id.as_deref(), group.as_deref()).await?;
            }
        }
        Commands::Wait { task_id } => {
            cmd::wait::run(&store, task_id.as_deref()).await?;
        }
        Commands::Board {
            running,
            today,
            mine,
            group,
        } => {
            cmd::board::run(&store, running, today, mine, group.as_deref())?;
        }
        Commands::Audit { task_id } => {
            cmd::audit::run(&store, &task_id)?;
        }
        Commands::Output { task_id } => {
            cmd::output::run(&store, &task_id)?;
        }
        Commands::Review { task_id } => {
            cmd::review::run(&store, cmd::review::ReviewArgs { task_id })?;
        }
        Commands::Usage => {
            cmd::usage::run(&store)?;
        }
        Commands::Retry { task_id, feedback } => {
            cmd::retry::run(store, cmd::retry::RetryArgs { task_id, feedback }).await?;
        }
        Commands::Explore {
            prompt,
            agent,
            model,
            files,
            output,
        } => {
            cmd::explore::run(store, prompt, agent, model, files, output).await?;
        }
        Commands::Mcp => {
            cmd::mcp::run(store).await?;
        }
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
        Commands::Agents => {
            let agents = agent::detect_agents();
            if agents.is_empty() {
                println!("No AI CLI agents detected.");
            } else {
                println!("Detected agents:");
                for a in &agents {
                    println!("  - {}", a.as_str());
                }
            }
        }
        Commands::InternalRunTask { task_id } => {
            background::run_task(store, &task_id).await?;
        }
    }

    Ok(())
}

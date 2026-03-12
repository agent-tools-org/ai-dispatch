// aid — Multi-AI CLI team orchestrator.
// Dispatches tasks to gemini/codex/opencode, watches progress, audits results.

mod agent;
mod board;
mod cmd;
mod cost;
mod paths;
mod store;
mod templates;
mod types;
mod watcher;

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
        /// Agent to use (gemini, codex, opencode)
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
        /// Run in background
        #[arg(long)]
        bg: bool,
    },
    /// Live progress dashboard
    Watch {
        /// Watch a specific task ID
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
    },
    /// Show detailed task audit
    Audit {
        /// Task ID to audit
        task_id: String,
    },
    /// Show detected agents
    Agents,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    paths::ensure_dirs()?;
    let store = Arc::new(store::Store::open(&paths::db_path())?);

    match cli.command {
        Commands::Run { agent, prompt, dir, output, model, bg } => {
            cmd::run::run(store, cmd::run::RunArgs {
                agent_name: agent,
                prompt,
                dir,
                output,
                model,
                background: bg,
            }).await?;
        }
        Commands::Watch { task_id } => {
            cmd::watch::run(&store, task_id.as_deref()).await?;
        }
        Commands::Board { running, today } => {
            cmd::board::run(&store, running, today)?;
        }
        Commands::Audit { task_id } => {
            cmd::audit::run(&store, &task_id)?;
        }
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
    }

    Ok(())
}

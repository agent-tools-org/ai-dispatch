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
mod notify;
mod paths;
mod prompt;
mod pty_bridge;
mod pty_runner;
mod pty_watch;
mod rate_limit;
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

/// Resolve group: CLI flag takes precedence, then AID_GROUP env var.
fn resolve_group(flag: Option<String>) -> Option<String> {
    flag.or_else(|| std::env::var("AID_GROUP").ok())
}

#[derive(Parser)]
#[command(name = "aid", version, about = "Multi-AI CLI team orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(after_help = r#"Examples:
  aid run codex "Add unit tests" --verify
  aid run gemini "Research topic" -o notes.md
  aid run codex "Refactor" -w feat/refactor --verify --retry 1 --bg"#)]
    /// Dispatch a task to an AI agent
    Run {
        /// Agent to use (auto, gemini, codex, opencode, cursor, kilo)
        agent: String,
        /// Prompt / task description
        prompt: String,
        /// Target repository path (defaults to current dir's git root)
        #[arg(long)]
        repo: Option<String>,
        /// Working directory for the agent
        #[arg(short, long)]
        dir: Option<String>,
        /// Output file path (for research tasks)
        #[arg(short, long)]
        output: Option<String>,
        /// Model override (for opencode)
        #[arg(short, long)]
        model: Option<String>,
        /// Prefer cheaper agents (avoid codex when quota is low)
        #[arg(long)]
        budget: bool,
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
        /// Prompt template to wrap around the task
        #[arg(long)]
        template: Option<String>,
        /// Disable automatic skill injection
        #[arg(long, conflicts_with = "skill")]
        no_skill: bool,
        /// Run in background
        #[arg(long)]
        bg: bool,
        /// Command to run on task completion
        #[arg(long)]
        on_done: Option<String>,
        /// Fallback agent if primary agent fails
        #[arg(long)]
        fallback: Option<String>,
        /// Run in read-only mode (no file writes)
        #[arg(long)]
        read_only: bool,
    },
    #[command(after_help = r#"Examples:
  aid batch tasks.toml --parallel
  aid batch tasks.toml --parallel --max-concurrent 3

Batch TOML format:
  [defaults]
  verify = true
  agent = "codex"

  [[task]]
  name = "types"
  prompt = "Create shared types"
  worktree = "feat/types""#)]
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
        /// Limit number of concurrent tasks
        #[arg(long)]
        max_concurrent: Option<usize>,
    },
    /// Benchmark a task across multiple agents
    Benchmark {
        /// Task prompt
        prompt: String,
        /// Comma-separated agents to compare
        #[arg(long)]
        agents: String,
        /// Working directory
        #[arg(short, long)]
        dir: Option<String>,
        /// Verify command
        #[arg(long, num_args = 0..=1, default_missing_value = "auto")]
        verify: Option<String>,
    },
    #[command(after_help = r#"Examples:
  aid watch t-1234               # Live TUI for one task
  aid watch --quiet t-1234       # Block until done (for scripts)
  aid watch --quiet --group wg-a # Block until group finishes
  aid watch --tui                # Full dashboard TUI"#)]
    /// Live progress / blocking wait (--quiet)
    Watch {
        /// Watch specific task IDs (multiple allowed)
        task_ids: Vec<String>,
        /// Restrict to one workgroup in multi-task mode
        #[arg(long)]
        group: Option<String>,
        /// Interactive TUI mode
        #[arg(long)]
        tui: bool,
        /// Silent blocking wait (replaces `aid wait`)
        #[arg(long)]
        quiet: bool,
        /// Exit when task enters AwaitingInput, print task ID and prompt
        #[arg(long)]
        exit_on_await: bool,
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
        /// Stream task status updates to stdout
        #[arg(short, long)]
        stream: bool,
    },
    /// Print the most recent completion notifications
    Completions,
    /// Clean up old tasks and orphaned worktrees
    Clean {
        /// Only clean tasks older than N days (default: 7)
        #[arg(long, default_value = "7")]
        older_than: u64,
        /// Also remove orphaned worktrees from /tmp/aid-wt-*
        #[arg(long)]
        worktrees: bool,
        /// Dry run — show what would be cleaned without deleting
        #[arg(long)]
        dry_run: bool,
    },
    #[command(after_help = r#"Examples:
  aid show t-1234              # Events timeline
  aid show t-1234 --diff       # Full worktree diff
  aid show t-1234 --output     # Task output
  aid show t-1234 --context    # Resolved prompt
  aid show t-1234 --explain    # AI explanation"#)]
    /// Inspect task artifacts (events, diff, output, explain)
    Show {
        /// Task ID to inspect
        task_id: String,
        /// Show the full resolved prompt sent to the agent
        #[arg(long, help = "Show the full resolved prompt sent to the agent")]
        context: bool,
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
    #[command(after_help = r#"Examples:
  aid retry t-1234 -f "Fix the compilation error in parser.rs"
  aid retry t-1234 -f "Use HashMap instead" --agent opencode"#)]
    /// Retry a failed task with feedback (optionally switch agent)
    Retry {
        /// Task ID to retry
        task_id: String,
        /// Feedback or correction for the agent
        #[arg(short, long)]
        feedback: String,
        /// Switch to a different agent (e.g. --agent opencode when codex is rate-limited)
        #[arg(long)]
        agent: Option<String>,
    },
    /// Mark task(s) as merged
    Merge {
        /// Task ID to mark as merged
        task_id: Option<String>,
        /// Mark all done tasks in a workgroup as merged
        #[arg(long)]
        group: Option<String>,
    },
    /// Send interactive input to a background task
    Respond {
        /// Task ID of the background task
        task_id: String,
        /// Response text (if omitted, reads from stdin)
        input: Option<String>,
        #[arg(long, short)]
        file: Option<String>,
    },
    #[command(after_help = r#"Examples:
  aid ask "What is the latest Rust edition?"
  aid ask "Explain this error" --files src/main.rs -o explanation.md"#)]
    /// Research/explore via cheap AI CLIs
    Ask {
        /// Question or research prompt
        prompt: String,
        /// Agent to use (default: gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(short, long)]
        model: Option<String>,
        /// Files to include as context
        #[arg(long)]
        files: Vec<String>,
        /// Output file for the response
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
    /// Initialize default skills and templates
    Init,
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
            repo,
            dir,
            output,
            model,
            budget,
            worktree,
            group,
            verify,
            retry,
            context,
            skill,
            template,
            no_skill,
            bg,
            on_done,
            fallback,
            read_only,
        } => {
            let config = config::load_config().unwrap_or_default();
            let budget = budget || config.selection.budget_mode;
            let agent_name = if agent == "auto" {
                let selection_opts = agent::RunOpts {
                    dir: dir
                        .clone()
                        .or_else(|| repo.clone())
                        .or_else(|| worktree.as_ref().map(|_| ".".to_string())),
                    output: output.clone(),
                    model: model.clone(),
                    budget,
                    read_only,
                    context_files: vec![],
                    session_id: None,
                };
                let (selected, reason) = agent::select_agent_with_reason(&prompt, &selection_opts, &store);
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
                    repo,
                    dir,
                    output,
                    model,
                    worktree,
                    base_branch: None,
                    group: resolve_group(group),
                    verify,
                    max_duration_mins: None,
                    retry,
                    context,
                    skills,
                    template,
                    background: bg,
                    announce: true,
                    on_done,
                    fallback,
                    read_only,
                    budget,
                    ..Default::default()
                },
            )
            .await?;
        }
        Commands::Batch {
            file,
            parallel,
            wait,
            max_concurrent,
        } => {
            cmd::batch::run(
                store,
                cmd::batch::BatchArgs {
                    file,
                    parallel,
                    wait,
                    max_concurrent,
                },
            )
            .await?;
        }
        Commands::Benchmark {
            prompt,
            agents,
            dir,
            verify,
        } => {
            cmd::benchmark::run(store, prompt, agents, dir, verify).await?;
        }
        Commands::Watch {
            task_ids,
            group,
            tui: true,
            ..
        } => {
            tui::run(&store, tui::RunOptions { task_id: task_ids.first().cloned(), group: resolve_group(group) })?;
        }
        Commands::Watch {
            task_ids,
            group,
            tui: false,
            quiet,
            exit_on_await,
        } => {
            let group = resolve_group(group);
            cmd::watch::run(&store, &task_ids, group.as_deref(), quiet, exit_on_await).await?;
        }
        Commands::Board {
            running,
            today,
            mine,
            group,
            stream,
        } => {
            let group = resolve_group(group);
            if stream {
                cmd::board_stream::run(&store, running, today, mine, group.as_deref()).await?;
            } else {
                cmd::board::run(&store, running, today, mine, group.as_deref())?;
            }
        }
        Commands::Completions => {
            let text = notify::read_recent(20)?;
            if !text.is_empty() {
                println!("{text}");
            }
        }
        Commands::Clean {
            older_than,
            worktrees,
            dry_run,
        } => {
            cmd::clean::run(store, older_than, worktrees, dry_run)?;
        }
        Commands::Show {
            task_id,
            context,
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
                    context,
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
        Commands::Retry { task_id, feedback, agent } => {
            cmd::retry::run(store, cmd::retry::RetryArgs { task_id, feedback, agent }).await?;
        }
        Commands::Merge { task_id, group } => {
            let group = resolve_group(group);
            cmd::merge::run(store, task_id.as_deref(), group.as_deref())?;
        }
        Commands::Respond {
            task_id,
            input,
            file,
        } => {
            cmd::respond::run(&task_id, input.as_deref(), file.as_deref())?;
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
        Commands::Init => cmd::init::run()?,
        Commands::InternalRunTask { task_id } => {
            background::run_task(store, &task_id).await?;
        }
    }

    Ok(())
}

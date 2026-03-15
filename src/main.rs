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
mod hooks;
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
use crate::cli_actions::{ConfigAction, GroupAction, WorktreeAction};
use crate::types::AgentKind;
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
        #[arg(long, short = 'g')]
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
        /// Hook specs to run for the dispatched task
        #[arg(long)]
        hook: Vec<String>,
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
    #[command(after_help = r#"Examples:
  aid agent list
  aid agent show aider
  aid agent add my-agent
  aid agent remove my-agent"#)]
    /// Manage custom agent definitions
    Agent {
        #[command(subcommand)]
        action: AgentCommands,
    },
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
    #[command(after_help = r#"Examples:
  aid export t-1234
  aid export t-1234 --format json --output task.json"#)]
    /// Export a task with full context
    Export {
        /// Task ID to export
        task_id: String,
        /// Output format (md or json)
        #[arg(long, default_value = "md")]
        format: String,
        /// Write export to a file
        #[arg(long)]
        output: Option<String>,
    },
    /// Show task-history usage and configured cost budgets
    Usage {
        /// Filter to current caller session
        #[arg(long)]
        session: bool,
        /// Show analytics for a specific agent (e.g. codex, gemini)
        #[arg(long)]
        agent: Option<String>,
        /// Time window to summarize (today, 7d, 30d, all)
        #[arg(long, default_value = "all")]
        period: String,
        /// Output raw JSON for automation
        #[arg(long)]
        json: bool,
    },
    /// Summarize workgroup results
    Summary {
        /// Workgroup ID (e.g. wg-abc1)
        group: String,
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
    #[command(after_help = r#"Examples:
  aid query "What does gamma=0 mean in CryptoSwap?"
  aid query "Explain this" --auto
  aid query "Key insight" -g wg-abc1 --finding"#)]
    /// Fast LLM query via OpenRouter (no agent startup)
    Query {
        /// Question to ask
        prompt: String,
        /// Use auto-tier model (paid, better quality)
        #[arg(short, long)]
        auto: bool,
        /// Explicit model override
        #[arg(short, long)]
        model: Option<String>,
        /// Workgroup ID for context
        #[arg(short, long)]
        group: Option<String>,
        /// Save response as a workgroup finding
        #[arg(long)]
        finding: bool,
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
    #[command(after_help = r#"Examples:
  aid worktree create feat/my-feature
  aid worktree create fix/bug --base develop
  aid worktree list
  aid worktree remove feat/my-feature"#)]
    /// Manage git worktrees for isolated task execution
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
    #[command(after_help = r#"Examples:
  aid store browse
  aid store install sunoj/aider
  aid store show sunoj/aider"#)]
    /// Browse and install agents from the community store
    Store {
        #[command(subcommand)]
        action: StoreCommands,
    },
    #[command(after_help = r#"Examples:
  aid memory add discovery \"The auth module uses bcrypt not argon2\"
  aid memory list --type convention
  aid memory search \"auth\"
  aid memory forget m-a3f1"#)]
    /// Manage agent shared memory (discoveries, conventions, lessons)
    Memory {
        #[command(subcommand)]
        action: MemoryCommands,
    },
    #[command(after_help = r#"Examples:
  aid finding add wg-abc1 "gamma can be zero in tricrypto"
  aid finding list wg-abc1"#)]
    /// Post or list workgroup findings
    Finding {
        #[command(subcommand)]
        action: FindingCommands,
    },
    /// Send a message to a workgroup's broadcast channel
    Broadcast {
        /// Workgroup ID
        group: String,
        /// Message to broadcast
        message: String,
    },
    /// Initialize default skills and templates
    Init,
    #[command(hide = true, name = "__run-task")]
    InternalRunTask { task_id: String },
    /// Show task retry tree
    Tree {
        /// Task ID
        task_id: String,
    },
    /// Print task output (shortcut for `show --output`)
    Output {
        /// Task ID
        task_id: String,
    },
}

#[derive(Subcommand)]
enum AgentCommands {
    /// List all agents (built-in + custom)
    List,
    /// Show agent details and configuration
    Show { name: String },
    /// Create a new custom agent definition
    Add { name: String },
    /// Remove a custom agent definition
    Remove { name: String },
    /// Fork a built-in or custom agent definition for local editing
    Fork {
        /// Name of the agent to fork (built-in or custom)
        name: String,
        /// Override the new agent name (defaults to `<name>-custom`)
        #[arg(long = "as")]
        new_name: Option<String>,
    },
}

#[derive(Subcommand)]
enum StoreCommands {
    /// Browse available agents in the store
    Browse {
        /// Optional search query to filter agents
        query: Option<String>,
    },
    /// Install an agent from the store (publisher/name)
    Install { name: String },
    /// Show agent TOML from the store (publisher/name)
    Show { name: String },
}

#[derive(Subcommand)]
enum MemoryCommands {
    /// Add a memory entry
    Add {
        /// Memory type: discovery, convention, lesson, fact
        #[arg(name = "TYPE")]
        memory_type: String,
        /// Content to remember
        content: String,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// List memories (project-scoped by default)
    List {
        /// Filter by type
        #[arg(long = "type")]
        memory_type: Option<String>,
        /// Show all memories across all projects
        #[arg(long)]
        all: bool,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// Search memories by keyword
    Search {
        /// Search query
        query: String,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// Update a memory's content
    Update {
        /// Memory ID (e.g. m-a3f1)
        id: String,
        /// New content
        content: String,
    },
    /// Delete a memory entry
    Forget {
        /// Memory ID (e.g. m-a3f1)
        id: String,
    },
}

#[derive(Subcommand)]
enum FindingCommands {
    /// Post a finding to a workgroup
    Add {
        /// Workgroup ID
        group: String,
        /// Finding content
        content: String,
        /// Source task ID (optional)
        #[arg(long)]
        task: Option<String>,
    },
    /// List findings for a workgroup
    List {
        /// Workgroup ID
        group: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    paths::ensure_dirs()?;
    let store = Arc::new(store::Store::open(&paths::db_path())?);
    let _ = background::check_zombie_tasks(&store);

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
            hook,
            read_only,
        } => {
            let config = config::load_config().unwrap_or_default();
            let budget = budget || config.selection.budget_mode;
            let (agent_name, auto_model) = if agent == "auto" {
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
                let rec = if model.is_none() && !budget {
                    let norm = prompt.trim().to_lowercase();
                    let fc = agent::classifier::count_file_mentions(&norm);
                    let profile = agent::classifier::classify(&prompt, fc, prompt.len());
                    let m = AgentKind::parse_str(&selected)
                        .and_then(|kind| agent::selection::recommend_model(&kind, &profile.complexity, false));
                    if let Some(name) = m {
                        eprintln!("[aid] Auto-selected model: {name} (complexity: {})", profile.complexity.label());
                    }
                    m.map(|s| s.to_string())
                } else {
                    None
                };
                (selected.clone(), rec)
            } else {
                (agent, None)
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
                    model: model.or(auto_model),
                    worktree,
                    base_branch: None,
                    group: resolve_group(group),
                    verify,
                    max_duration_mins: None,
                    retry,
                    context,
                    skills,
                    hooks: hook,
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
        Commands::Agent { action } => {
            use cmd::agent::{AgentAction, run_agent_command};
            let action = match action {
                AgentCommands::List => AgentAction::List,
                AgentCommands::Show { name } => AgentAction::Show { name },
                AgentCommands::Add { name } => AgentAction::Add { name },
                AgentCommands::Remove { name } => AgentAction::Remove { name },
                AgentCommands::Fork { name, new_name } => AgentAction::Fork { name, new_name },
            };
            run_agent_command(action)?;
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
        Commands::Export {
            task_id,
            format,
            output,
        } => {
            let format = cmd::export::ExportFormat::parse(&format)?;
            cmd::export::run(
                store.clone(),
                cmd::export::ExportArgs {
                    task_id,
                    format,
                    output,
                },
            )
            .await?;
        }
        Commands::Tree { task_id } => {
            cmd::tree::run(&store, &task_id)?;
        }
        Commands::Output { task_id } => {
            let store = store::Store::open(&paths::db_path())?;
            let text = cmd::show::output_text_for_task(&store, &task_id)?;
            print!("{text}");
        }
        Commands::Usage { session, agent, period, json } => {
            cmd::usage::run(&store, session, agent, period, json)?;
        }
        Commands::Summary { group } => {
            cmd::summary::run(&store, &group)?;
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
        Commands::Query { prompt, auto, model, group, finding } => {
            let group = group.or_else(|| resolve_group(None));
            cmd::query::run(&store, &prompt, model.as_deref(), auto, group.as_deref(), finding)?;
        }
        Commands::Mcp => cmd::mcp::run(store).await?,
        Commands::Config { action } => {
            cmd::config::run(&store, action)?;
        }
        Commands::Group { action } => match action {
            GroupAction::Create { name, context } => cmd::group::create(&store, &name, context.as_deref().unwrap_or(""))?,
            GroupAction::List => cmd::group::list(&store)?,
            GroupAction::Show { group_id } => cmd::group::show(&store, &group_id)?,
            GroupAction::Update {
                group_id,
                name,
                context,
            } => cmd::group::update(&store, &group_id, name.as_deref(), context.as_deref())?,
            GroupAction::Delete { group_id } => cmd::group::delete(&store, &group_id)?,
        },
        Commands::Worktree { action } => match action {
            WorktreeAction::Create { branch, base, repo } => {
                cmd::worktree::create(&branch, base.as_deref(), repo.as_deref())?;
            }
            WorktreeAction::List { repo } => {
                cmd::worktree::list(repo.as_deref())?;
            }
            WorktreeAction::Remove { branch, repo } => {
                cmd::worktree::remove(&branch, repo.as_deref())?;
            }
        },
        Commands::Store { action } => {
            use cmd::store::{StoreAction, run_store};
            let action = match action {
                StoreCommands::Browse { query } => StoreAction::Browse { query },
                StoreCommands::Install { name } => StoreAction::Install { name },
                StoreCommands::Show { name } => StoreAction::Show { name },
            };
            run_store(action)?;
        }
        Commands::Memory { action } => match action {
            MemoryCommands::Add {
                memory_type,
                content,
                project,
            } => {
                cmd::memory::add(&store, &memory_type, &content, project.as_deref())?;
            }
            MemoryCommands::List { memory_type, all, project } => {
                cmd::memory::list(&store, memory_type.as_deref(), project.as_deref(), all)?;
            }
            MemoryCommands::Search { query, project } => {
                cmd::memory::search(&store, &query, project.as_deref())?;
            }
            MemoryCommands::Update { id, content } => {
                cmd::memory::update(&store, &id, &content)?;
            }
            MemoryCommands::Forget { id } => {
                cmd::memory::forget(&store, &id)?;
            }
        },
        Commands::Finding { action } => match action {
            FindingCommands::Add { group, content, task } => {
                cmd::finding::add(&store, &group, &content, task.as_deref())?;
            }
            FindingCommands::List { group } => {
                cmd::finding::list(&store, &group)?;
            }
        },
        Commands::Broadcast { group, message } => {
            cmd::broadcast::run(&store, &group, &message)?;
        }
        Commands::Init => cmd::init::run()?,
        Commands::InternalRunTask { task_id } => {
            background::run_task(store, &task_id).await?;
        }
    }

    Ok(())
}

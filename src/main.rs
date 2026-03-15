// aid — Multi-AI CLI team orchestrator.
// Entry point wiring: modules, CLI dispatch, and exported main.

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
#[cfg(feature = "evermemos")]
mod evermemos;
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
mod team;
mod templates;
mod compaction;
mod tui;
mod types;
mod usage;
pub mod usage_report;
mod verify;
mod watcher;
mod webhook;
mod workgroup;
mod worktree;
mod cli;

use crate::cli_actions::{GroupAction, TeamAction, WorktreeAction};
use crate::types::AgentKind;
use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use crate::cli::{Cli, Commands, AgentCommands, StoreCommands, MemoryCommands, FindingCommands};

/// Resolve group: CLI flag takes precedence, then AID_GROUP env var.
fn resolve_group(flag: Option<String>) -> Option<String> {
    flag.or_else(|| std::env::var("AID_GROUP").ok())
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
            team: team_flag,
            group,
            verify,
            retry,
            context,
            context_from,
            skill,
            template,
            no_skill,
            bg,
            on_done,
            cascade,
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
                let team_config = team_flag.as_deref().and_then(team::resolve_team);
                let (selected, reason) = agent::select_agent_with_reason(
                    &prompt, &selection_opts, &store, team_config.as_ref(),
                );
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
                    cascade,
                    read_only,
                    budget,
                    team: team_flag,
                    context_from,
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
            json,
        } => {
            let group = resolve_group(group);
            if stream {
                cmd::board_stream::run(&store, running, today, mine, group.as_deref()).await?;
            } else {
                cmd::board::run(&store, running, today, mine, group.as_deref(), json)?;
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
            events: _,
            context,
            diff,
            output,
            explain,
            log,
            json,
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
                    json,
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
        Commands::Usage { session, agent, team: team_flag, period, json } => {
            cmd::usage::run(&store, session, agent, team_flag, period, json)?;
        }
        Commands::Summary { group } => {
            cmd::summary::run(&store, &group)?;
        }
        Commands::Retry { task_id, feedback, agent, dir } => {
            cmd::retry::run(store, cmd::retry::RetryArgs { task_id, feedback, agent, dir }).await?;
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
                StoreCommands::Update { apply } => StoreAction::Update { apply },
            };
            run_store(action)?;
        }
        Commands::Team { action } => {
            use cmd::team::{TeamAction as TA, run_team_command};
            let action = match action {
                TeamAction::List => TA::List,
                TeamAction::Show { name } => TA::Show { name },
                TeamAction::Create { name } => TA::Create { name },
                TeamAction::Delete { name } => TA::Delete { name },
            };
            run_team_command(action)?;
        }
        Commands::Memory { action } => match action {
            MemoryCommands::Add {
                memory_type,
                content,
                project,
            } => {
                cmd::memory::add(&store, &memory_type, &content, project.as_deref())?;
            }
            MemoryCommands::List { memory_type, all, project, stats } => {
                cmd::memory::list(&store, memory_type.as_deref(), project.as_deref(), all, stats)?;
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
            MemoryCommands::History { id } => {
                cmd::memory::history(&store, &id)?;
            }
            #[cfg(feature = "evermemos")]
            MemoryCommands::CloudStatus => {
                cmd::memory::cloud_status()?;
            }
            #[cfg(feature = "evermemos")]
            MemoryCommands::CloudSearch { query, limit } => {
                cmd::memory::cloud_search(&query, limit)?;
            }
            #[cfg(feature = "evermemos")]
            MemoryCommands::CloudPush { memory_type } => {
                cmd::memory::cloud_push(&store, memory_type.as_deref())?;
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
        Commands::Upgrade { force } => {
            cmd::upgrade::run(force)?;
        }
        Commands::Init => cmd::init::run()?,
        Commands::Setup => cmd::setup::run()?,
        Commands::InternalRunTask { task_id } => {
            background::run_task(store, &task_id).await?;
        }
    }

    Ok(())
}

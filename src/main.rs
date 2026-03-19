// aid — Multi-AI CLI team orchestrator.
// Entry point wiring: modules, CLI dispatch, and exported main.

#[macro_use]
pub mod output;
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
mod process_guard;
mod pty_bridge;
mod pty_runner;
mod pty_watch;
mod rate_limit;
pub(crate) mod sanitize;
mod session;
mod skills;
mod store;
mod store_workgroups;
mod project;
mod team;
mod templates;
#[cfg(test)]
mod test_subprocess;
mod compaction;
pub mod claudemd;
mod tui;
mod system_resources;
mod types;
mod update_check;
mod usage;
pub mod usage_report;
mod verify;
mod watcher;
mod webhook;
#[cfg(feature = "web")]
mod web;
mod workgroup;
mod worktree;
mod cli;
use crate::cli_actions::{GroupAction, GroupFindingAction, ProjectAction, TeamAction, WorktreeAction};
use crate::types::AgentKind;
use anyhow::{Result, bail};
use clap::Parser;
use std::fs;
use std::io::{IsTerminal, Read};
use std::sync::Arc;
use crate::cli::{BatchAction, Cli, Commands, AgentCommands, HookAction, StoreCommands, MemoryCommands, FindingCommands};
use crate::cmd::experiment_types::{ExperimentConfig, MetricDirection};

/// Resolve group: CLI flag takes precedence, then AID_GROUP env var.
fn resolve_group(flag: Option<String>) -> Option<String> {
    flag.or_else(|| std::env::var("AID_GROUP").ok())
}

fn resolve_finding_content(content: Option<String>, stdin: bool, file: Option<String>) -> Result<String> {
    let stdin_is_terminal = std::io::stdin().is_terminal();
    resolve_finding_content_from(content, stdin, file, stdin_is_terminal, &mut std::io::stdin())
}

fn resolve_finding_content_from<R: Read>(
    content: Option<String>,
    stdin: bool,
    file: Option<String>,
    stdin_is_terminal: bool,
    reader: &mut R,
) -> Result<String> {
    if let Some(path) = file {
        return Ok(fs::read_to_string(path)?);
    }
    if stdin || (content.is_none() && !stdin_is_terminal) {
        let mut buffer = String::new();
        reader.read_to_string(&mut buffer)?;
        return Ok(buffer);
    }
    if let Some(content) = content {
        return Ok(content);
    }
    bail!("No finding content provided")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    output::init();
    if cli.quiet {
        output::set_quiet(true);
    }

    paths::ensure_dirs()?;
    let config = config::load_config().unwrap_or_default();
    if config.updates.check {
        update_check::maybe_check_update();
    }
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
            scope,
            run_extras,
            no_skill,
            bg,
            dry_run,
            read_only,
            judge,
            peer_review,
            best_of,
            metric,
            parent,
            id,
            timeout,
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
                    env: None,
                    env_forward: None,
                };
                let team_config = team_flag.as_deref().and_then(team::resolve_team);
                let (selected, reason) = agent::select_agent_with_reason(
                    &prompt, &selection_opts, &store, team_config.as_ref(),
                );
                aid_info!("[aid] Auto-selected: {selected} (reason: {reason})");
                let rec = if model.is_none() && !budget {
                    let norm = prompt.trim().to_lowercase();
                    let fc = agent::classifier::count_file_mentions(&norm);
                    let profile = agent::classifier::classify(&prompt, fc, prompt.len());
                    let m = AgentKind::parse_str(&selected)
                        .and_then(|kind| agent::selection::recommend_model(&kind, &profile.complexity, false));
                    m.map(|s| s.to_string())
                } else {
                    None
                };
                (selected.clone(), rec)
            } else {
                (agent, None)
            };
            let extras = *run_extras;
            let skills = if no_skill {
                vec![cmd::run::NO_SKILL_SENTINEL.to_string()]
            } else {
                extras.skill
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
                    hooks: extras.hook,
                    template: extras.template,
                    background: bg,
                    dry_run,
                    announce: true,
                    on_done: extras.on_done,
                    cascade: extras.cascade,
                    read_only,
                    budget,
                    judge,
                    peer_review,
                    best_of,
                    metric,
                    team: team_flag,
                    context_from: extras.context_from,
                    scope,
                    parent_task_id: parent,
                    existing_task_id: id.map(crate::types::TaskId),
                    timeout,
                    ..Default::default()
                },
            )
            .await?;
        }
        Commands::Batch {
            action,
            file,
            vars,
            parallel,
            wait,
            dry_run,
            max_concurrent,
            output,
        } => {
            match action {
                Some(BatchAction::Init) => cmd::batch::init(output.as_deref())?,
                Some(BatchAction::Retry { group_id, agent }) => {
                    cmd::batch::retry_failed(store, &group_id, agent.as_deref()).await?;
                }
                None => {
                    let file = file.ok_or_else(|| anyhow::anyhow!("batch file is required"))?;
                    cmd::batch::run(
                        store,
                        cmd::batch::BatchArgs {
                            file,
                            vars,
                            parallel,
                            wait,
                            dry_run,
                            max_concurrent,
                        },
                    )
                    .await?;
                }
            }
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
            timeout,
        } => {
            let group = resolve_group(group);
            if quiet {
                cmd::wait::run(&store, &task_ids, group.as_deref(), exit_on_await, timeout).await?;
            } else {
                cmd::watch::run(&store, &task_ids, group.as_deref(), quiet, exit_on_await).await?;
            }
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
        Commands::Changelog {
            version,
            all,
            count,
            git,
        } => {
            cmd::changelog::run(version, all, count, git)?;
        }
        Commands::Agent { action } => {
            use cmd::agent::{AgentAction, run_agent_command};
            let action = match action {
                AgentCommands::List => AgentAction::List,
                AgentCommands::Show { name } => AgentAction::Show { name },
                AgentCommands::Add { name } => AgentAction::Add { name },
                AgentCommands::Remove { name } => AgentAction::Remove { name },
                AgentCommands::Fork { name, new_name } => AgentAction::Fork { name, new_name },
                AgentCommands::Quota => AgentAction::Quota,
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
            summary,
            file,
            output,
            full,
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
                    summary,
                    file,
                    output,
                    full,
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
        Commands::Output { task_id, full } => {
            let store = store::Store::open(&paths::db_path())?;
            let text = cmd::show::output_text_for_task(&store, &task_id, full)?;
            print!("{text}");
        }
        Commands::Usage { session, agent, team: team_flag, period, json } => {
            cmd::usage::run(&store, session, agent, team_flag, period, json)?;
        }
        Commands::Cost {
            group,
            summary,
            agent,
            period,
        } => {
            cmd::cost::run(&store, group, summary, agent, period)?;
        }
        Commands::Summary { group } => {
            cmd::summary_cli::run(&store, &group)?;
        }
        Commands::Retry {
            task_id,
            feedback,
            agent,
            dir,
            reset,
        } => {
            cmd::retry::run(
                store,
                cmd::retry::RetryArgs {
                    task_id,
                    feedback,
                    agent,
                    dir,
                    reset,
                },
            )
            .await?;
        }
        Commands::Merge {
            task_id,
            group,
            approve,
        } => {
            let group = resolve_group(group);
            cmd::merge::run(store, task_id.as_deref(), group.as_deref(), approve)?;
        }
        Commands::Respond {
            task_id,
            input,
            file,
        } => {
            cmd::respond::run(&task_id, input.as_deref(), file.as_deref())?;
        }
        Commands::Stop { task_id, force } => {
            if force {
                cmd::stop::kill(&store, &task_id)?;
            } else {
                cmd::stop::stop(&store, &task_id)?;
            }
        }
        Commands::Kill { task_id } => {
            cmd::stop::kill(&store, &task_id)?;
        }
        Commands::Steer { task_id, message } => {
            cmd::steer::run(&store, &task_id, &message)?;
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
        Commands::Hook { action } => match action {
            HookAction::SessionStart => cmd::hook::session_start()?,
        },
        Commands::Config { action } => {
            cmd::config::run(&store, action)?;
        }
        Commands::Group { action } => match action {
            GroupAction::Create { name, context, id } => cmd::group::create(&store, &name, context.as_deref().unwrap_or(""), id.as_deref())?,
            GroupAction::List => cmd::group::list(&store)?,
            GroupAction::Show { group_id } => cmd::group::show(&store, &group_id)?,
            GroupAction::Update {
                group_id,
                name,
                context,
            } => cmd::group::update(&store, &group_id, name.as_deref(), context.as_deref())?,
            GroupAction::Delete { group_id } => cmd::group::delete(&store, &group_id)?,
            GroupAction::Summary { group_id } => cmd::summary_cli::run(&store, &group_id)?,
            GroupAction::Finding { action } => match action {
                GroupFindingAction::Add {
                    group,
                    content,
                    stdin,
                    file,
                    task,
                    severity,
                    title,
                    finding_file,
                    lines,
                    category,
                    confidence,
                } => {
                    let content = resolve_finding_content(content, stdin, file)?;
                    cmd::finding::add(
                        &store,
                        &group,
                        &content,
                        task.as_deref(),
                        severity.as_deref(),
                        title.as_deref(),
                        finding_file.as_deref(),
                        lines.as_deref(),
                        category.as_deref(),
                        confidence.as_deref(),
                    )?;
                }
                GroupFindingAction::List { group, json, count } => {
                    cmd::finding::list(&store, &group, json, count)?;
                }
            },
            GroupAction::Broadcast { group_id, message } => {
                cmd::broadcast::run(&store, &group_id, &message)?;
            }
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
        Commands::Project { action } => {
            use cmd::project::{ProjectAction as ProjectCommand, run_project_command};
            let action = match action {
                ProjectAction::Init => ProjectCommand::Init,
                ProjectAction::Show => ProjectCommand::Show,
                ProjectAction::Sync => ProjectCommand::Sync,
            };
            run_project_command(action)?;
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
        },
        Commands::Finding { action } => match action {
            FindingCommands::Add {
                group,
                content,
                stdin,
                file,
                task,
                severity,
                title,
                finding_file,
                lines,
                category,
                confidence,
            } => {
                let content = resolve_finding_content(content, stdin, file)?;
                cmd::finding::add(
                    &store,
                    &group,
                    &content,
                    task.as_deref(),
                    severity.as_deref(),
                    title.as_deref(),
                    finding_file.as_deref(),
                    lines.as_deref(),
                    category.as_deref(),
                    confidence.as_deref(),
                )?;
            }
            FindingCommands::List { group, json, count } => {
                cmd::finding::list(&store, &group, json, count)?;
            }
        },
        Commands::Broadcast { group, message } => {
            cmd::broadcast::run(&store, &group, &message)?;
        }
        Commands::Upgrade { force } => {
            cmd::upgrade::run(force)?;
        }
        #[cfg(feature = "web")]
        Commands::Web { port } => {
            cmd::web::run(port).await?;
        }
        Commands::Init => cmd::init::run()?,
        Commands::Setup => cmd::setup::run()?,
        Commands::InternalRunTask { task_id } => {
            background::run_task(store, &task_id).await?;
        }
        Commands::Experiment(sub) => {
            match sub {
                cli::ExperimentCommands::Run { agent, prompt, metric, direction, checks, max_runs, worktree, verify } => {
                    let config = ExperimentConfig {
                        metric_command: metric,
                        direction: match direction.to_lowercase().as_str() {
                            "min" => MetricDirection::Min,
                            _ => MetricDirection::Max,
                        },
                        agent, prompt, checks,
                        max_runs: Some(max_runs),
                        worktree, verify,
                    };
                    cmd::experiment::run_experiment(store.clone(), config).await?;
                }
                cli::ExperimentCommands::Status { dir } => {
                    cmd::experiment_status::run_status(dir.as_deref())?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_finding_content_from;
    use std::fs;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolve_finding_content_prefers_file() {
        let path = std::env::temp_dir().join(format!(
            "aid-finding-{}.txt",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "from file").unwrap();
        let mut stdin = Cursor::new("from stdin");

        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            true,
            Some(path.to_string_lossy().into_owned()),
            false,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(content, "from file");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn resolve_finding_content_reads_stdin_when_requested() {
        let mut stdin = Cursor::new("from stdin");

        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            true,
            None,
            true,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(content, "from stdin");
    }

    #[test]
    fn resolve_finding_content_reads_stdin_when_piped_without_arg() {
        let mut stdin = Cursor::new("from pipe");

        let content = resolve_finding_content_from(None, false, None, false, &mut stdin).unwrap();

        assert_eq!(content, "from pipe");
    }

    #[test]
    fn resolve_finding_content_uses_inline_arg() {
        let mut stdin = Cursor::new("");

        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            false,
            None,
            true,
            &mut stdin,
        )
        .unwrap();

        assert_eq!(content, "inline");
    }

    #[test]
    fn resolve_finding_content_errors_without_input() {
        let mut stdin = Cursor::new("");

        let error = resolve_finding_content_from(None, false, None, true, &mut stdin).unwrap_err();

        assert_eq!(error.to_string(), "No finding content provided");
    }
}

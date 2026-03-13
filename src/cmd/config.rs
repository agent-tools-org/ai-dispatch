// Handler for `aid config` — agent configuration and detection.
// Manages agent registry and displays detected AI CLIs.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

use crate::agent;
use crate::cli_actions::ConfigAction;
use crate::cost;
use crate::rate_limit;
use crate::skills;
use crate::store::Store;
use crate::templates;
use crate::types::{AgentKind, TaskFilter, TaskStatus};

const AGENT_PROFILES: &[(AgentKind, &str, &str, &str, bool)] = &[
    (
        AgentKind::Gemini,
        "Research, fact-checking, documentation, web search",
        "$0.10-$10/M blended",
        "research, explain, what is, how does, find, analyze",
        false,
    ),
    (
        AgentKind::Codex,
        "Complex implementation, multi-file refactors, test suites",
        "$0.10-$8/M blended",
        "implement, create, build, refactor, test",
        true,
    ),
    (
        AgentKind::OpenCode,
        "Simple edits, renames, type annotations, quick fixes",
        "free-$2/M blended",
        "rename, change, update, fix typo, add type",
        true,
    ),
    (
        AgentKind::Kilo,
        "Simple edits, renames, type annotations (free tier)",
        "free",
        "rename, change, update, fix typo, add type",
        true,
    ),
    (
        AgentKind::Cursor,
        "Frontend, UI components, responsive layouts",
        "varies",
        "ui, frontend, css, html, react, component, layout",
        true,
    ),
];

const MODEL_PRICING: &[(&str, f64, f64, &str)] = &[
    ("gpt-4.1", 2.0, 8.0, "Codex default"),
    ("gpt-4.1-mini", 0.4, 1.6, "Codex cheap"),
    ("gpt-4.1-nano", 0.1, 0.4, "Codex ultra-cheap"),
    ("gemini-2.5-flash", 0.15, 0.60, "Gemini default"),
    ("gemini-2.5-pro", 1.25, 10.0, "Gemini pro"),
    ("glm-5", 0.5, 2.0, "OpenCode (paid)"),
    ("kimi-k2.5", 0.5, 2.0, "OpenCode (paid)"),
    ("mimo-v2-flash-free", 0.0, 0.0, "OpenCode free"),
    ("nemotron-3-super-free", 0.0, 0.0, "OpenCode free"),
    ("minimax-m2.5-free", 0.0, 0.0, "OpenCode free"),
];

pub fn run(store: &Arc<Store>, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Agents => {
            let installed = agent::detect_agents();
            let history = match store.list_tasks(TaskFilter::All) {
                Ok(tasks) => compute_agent_history(&tasks),
                Err(_) => HashMap::new(),
            };
            for (kind, _, _, _, _) in AGENT_PROFILES {
                let status = if installed.contains(kind) {
                    "✓"
                } else {
                    "✗"
                };
                let profile = agent_profile(*kind, installed.contains(kind), history.get(kind));
                println!("{} {}\n{}", status, kind.as_str(), profile);
            }
        }
        ConfigAction::Skills => {
            let skills = skills::list_skills()?;
            if skills.is_empty() {
                println!("No skills found in ~/.aid/skills/.");
                println!("  Run `aid init` to install default skills.");
            } else {
                println!("Available skills:");
                for skill in &skills {
                    println!("  - {skill}");
                }
            }
        }
        ConfigAction::Templates => {
            let templates = templates::list_templates();
            if templates.is_empty() {
                println!("No templates found in ~/.aid/templates/.");
                println!("  Run `aid init` to install default templates.");
            } else {
                println!("Available templates:");
                for template in &templates {
                    println!("  - {template}");
                }
            }
        }
        ConfigAction::Pricing => {
            println!(
                "{:<25} {:>12} {:>12} {:>12} Description",
                "Model", "Input/M", "Output/M", "Blended/M"
            );
            println!("{}", "-".repeat(75));
            for (model, input, output, desc) in MODEL_PRICING {
                let blended = input * 0.7 + output * 0.3;
                println!(
                    "{:<25} ${:>11.2} ${:>11.2} ${:>11.4} {}",
                    model, input, output, blended, desc
                );
            }
        }
        ConfigAction::AddAgent { .. } => {
            println!("Custom agent registration not yet implemented");
        }
    }
    Ok(())
}

fn agent_profile(kind: AgentKind, installed: bool, history: Option<&AgentHistory>) -> String {
    let profile = AGENT_PROFILES.iter().find(|(k, _, _, _, _)| *k == kind);
    let (strengths, cost, _best_for, streaming) = match profile {
        Some((_, s, c, b, st)) => (*s, *c, *b, *st),
        None => ("unknown", "unknown", "unknown", false),
    };
    let mode = if streaming { "streaming" } else { "buffered" };
    let install_status = if installed {
        "installed"
    } else {
        "not installed"
    };
    let history_line = match history {
        Some(h) => format!(
            "  History:   {} tasks, {:.1}% success, avg {}/task\n",
            h.task_count,
            h.success_rate,
            cost::format_cost(Some(h.avg_cost))
        ),
        None => "  History:   no tasks yet\n".to_string(),
    };
    let rate_limit_line = match rate_limit::get_rate_limit_info(&kind) {
        Some(info) if info.recovery_at.is_some() => {
            format!(
                "  Status:    rate-limited (try again at {})\n",
                info.recovery_at.unwrap()
            )
        }
        _ => "".to_string(),
    };
    format!(
        "  Strengths: {}\n  Cost:      {}\n{}{}  Mode:      {} ({})\n",
        strengths, cost, history_line, rate_limit_line, mode, install_status
    )
}

struct AgentHistory {
    task_count: usize,
    success_rate: f64,
    avg_cost: f64,
}

fn compute_agent_history(tasks: &[crate::types::Task]) -> HashMap<AgentKind, AgentHistory> {
    let mut history = HashMap::new();
    for agent in [
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::OpenCode,
        AgentKind::Cursor,
        AgentKind::Kilo,
    ] {
        let agent_tasks: Vec<_> = tasks.iter().filter(|t| t.agent == agent).collect();
        if agent_tasks.is_empty() {
            continue;
        }
        let done_count = agent_tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Merged))
            .count();
        let success_rate = (done_count as f64 / agent_tasks.len() as f64) * 100.0;
        let total_cost: f64 = agent_tasks.iter().filter_map(|t| t.cost_usd).sum();
        let avg_cost = total_cost / agent_tasks.len() as f64;
        history.insert(
            agent,
            AgentHistory {
                task_count: agent_tasks.len(),
                success_rate,
                avg_cost,
            },
        );
    }
    history
}

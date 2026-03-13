// Handler for `aid config` — agent configuration and detection.
// Manages agent registry and displays detected AI CLIs.

use anyhow::Result;
use std::sync::Arc;

use crate::agent;
use crate::cli_actions::ConfigAction;
use crate::skills;
use crate::store::Store;
use crate::types::AgentKind;

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

pub fn run(_store: &Arc<Store>, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Agents => {
            let installed = agent::detect_agents();
            for (kind, _, _, _, _) in AGENT_PROFILES {
                let status = if installed.contains(kind) {
                    "✓"
                } else {
                    "✗"
                };
                let profile = agent_profile(*kind, installed.contains(kind));
                println!("{} {}\n{}", status, kind.as_str(), profile);
            }
        }
        ConfigAction::Skills => {
            let skills = skills::list_skills()?;
            if skills.is_empty() {
                println!("No skills found in ~/.aid/skills/.");
            } else {
                println!("Available skills:");
                for skill in &skills {
                    println!("  - {skill}");
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

fn agent_profile(kind: AgentKind, installed: bool) -> String {
    let profile = AGENT_PROFILES.iter().find(|(k, _, _, _, _)| *k == kind);
    let (strengths, cost, best_for, streaming) = match profile {
        Some((_, s, c, b, st)) => (*s, *c, *b, *st),
        None => ("unknown", "unknown", "unknown", false),
    };
    let mode = if streaming { "streaming" } else { "buffered" };
    let install_status = if installed {
        "installed"
    } else {
        "not installed"
    };
    format!(
        "  Strengths: {}\n  Cost:      {}\n  Best for:  {}\n  Mode:      {} ({})\n",
        strengths, cost, best_for, mode, install_status
    )
}

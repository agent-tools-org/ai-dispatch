// Handler for `aid config` — agent configuration and detection.
// Manages agent registry and displays detected AI CLIs.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::Arc;

use crate::agent;
use crate::agent::custom::CapabilityScores;
use crate::agent::registry;
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
        "Research, coding, web search, file editing",
        "$0.10-$10/M blended",
        "research, explain, implement, create, analyze, build",
        true,
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
        "General coding, strong model selection, frontend",
        "$20/mo subscription",
        "implement, create, build, refactor, ui, frontend, css",
        true,
    ),
];

pub struct AgentModel {
    pub agent: AgentKind,
    pub model: &'static str,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub tier: &'static str,
    pub description: &'static str,
    pub capability: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct PricingResponse {
    models: Vec<PricingFileModel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PricingFileModel {
    pub agent: String,
    pub model: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub tier: String,
    pub description: String,
    pub updated: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedAgentModel {
    pub agent: AgentKind,
    pub model: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub tier: String,
    pub description: String,
}

pub const AGENT_MODELS: &[AgentModel] = &[
    AgentModel {
        agent: AgentKind::Codex,
        model: "gpt-5.4",
        input_per_m: 2.5,
        output_per_m: 15.0,
        tier: "premium",
        description: "Latest, best quality",
        capability: 9.4,
    },
    AgentModel {
        agent: AgentKind::Codex,
        model: "gpt-4.1",
        input_per_m: 2.0,
        output_per_m: 8.0,
        tier: "standard",
        description: "Reliable, good quality",
        capability: 8.7,
    },
    AgentModel {
        agent: AgentKind::Codex,
        model: "gpt-4.1-mini",
        input_per_m: 0.4,
        output_per_m: 1.6,
        tier: "cheap",
        description: "Balanced cost/quality",
        capability: 6.3,
    },
    AgentModel {
        agent: AgentKind::Codex,
        model: "gpt-4.1-nano",
        input_per_m: 0.1,
        output_per_m: 0.4,
        tier: "cheap",
        description: "Ultra-cheap, simple tasks",
        capability: 4.6,
    },
    AgentModel {
        agent: AgentKind::Gemini,
        model: "flash",
        input_per_m: 0.30,
        output_per_m: 2.50,
        tier: "cheap",
        description: "Fast, balanced (default)",
        capability: 7.3,
    },
    AgentModel {
        agent: AgentKind::Gemini,
        model: "pro",
        input_per_m: 1.25,
        output_per_m: 10.0,
        tier: "premium",
        description: "Complex reasoning",
        capability: 7.8,
    },
    AgentModel {
        agent: AgentKind::Gemini,
        model: "flash-lite",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "cheap",
        description: "Fastest, simple tasks",
        capability: 5.9,
    },
    AgentModel {
        agent: AgentKind::OpenCode,
        model: "glm-4.7",
        input_per_m: 0.38,
        output_per_m: 1.98,
        tier: "cheap",
        description: "Paid, good quality",
        capability: 6.5,
    },
    AgentModel {
        agent: AgentKind::OpenCode,
        model: "kimi-k2.5",
        input_per_m: 0.45,
        output_per_m: 2.20,
        tier: "cheap",
        description: "Paid, good quality",
        capability: 6.1,
    },
    AgentModel {
        agent: AgentKind::OpenCode,
        model: "mimo-v2-flash-free",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "free",
        description: "Free tier",
        capability: 4.3,
    },
    AgentModel {
        agent: AgentKind::OpenCode,
        model: "nemotron-3-super-free",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "free",
        description: "Free tier",
        capability: 4.1,
    },
    AgentModel {
        agent: AgentKind::OpenCode,
        model: "minimax-m2.5-free",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "free",
        description: "Free tier",
        capability: 4.1,
    },
    AgentModel {
        agent: AgentKind::Kilo,
        model: "default",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "free",
        description: "Free tier",
        capability: 3.8,
    },
    AgentModel {
        agent: AgentKind::Cursor,
        model: "auto",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "cheap",
        description: "Auto routing, cheapest (recommended)",
        capability: 7.0,
    },
    AgentModel {
        agent: AgentKind::Cursor,
        model: "composer-1.5",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "standard",
        description: "Cursor proprietary, multi-file refactoring",
        capability: 8.0,
    },
    AgentModel {
        agent: AgentKind::Cursor,
        model: "opus-4.6-thinking",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "premium",
        description: "Strongest reasoning, premium pool",
        capability: 9.2,
    },
    AgentModel {
        agent: AgentKind::Cursor,
        model: "gpt-5.4-high",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "premium",
        description: "GPT-5.4 High, premium pool",
        capability: 9.0,
    },
    AgentModel {
        agent: AgentKind::Codebuff,
        model: "auto",
        input_per_m: 0.0,
        output_per_m: 0.0,
        tier: "standard",
        description: "SDK-managed pricing",
        capability: 6.8,
    },
];

pub fn run(store: &Arc<Store>, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Agents => {
            let installed = agent::detect_agents();
            let (history, model_history) = match store.list_tasks(TaskFilter::All) {
                Ok(tasks) => (compute_agent_history(&tasks), compute_model_history(&tasks)),
                Err(_) => (HashMap::new(), HashMap::new()),
            };
            for (kind, _, _, _, _) in AGENT_PROFILES {
                let status = if installed.contains(kind) {
                    "✓"
                } else {
                    "✗"
                };
                let profile = agent_profile(*kind, installed.contains(kind), history.get(kind), &model_history);
                println!("{} {}\n{}", status, kind.as_str(), profile);
            }
            let custom_agents = registry::list_custom_agents();
            if custom_agents.is_empty() {
                println!("\nCustom agents: none found.");
            } else {
                println!("\nCustom agents:");
                for agent in custom_agents {
                    let install_status = if command_installed(&agent.command) {
                        "installed"
                    } else {
                        "not installed"
                    };
                    println!("  - Name: {}", agent.id);
                    println!("    Display name: {}", agent.display_name);
                    println!("    Command: {} ({})", agent.command, install_status);
                    println!(
                        "    Capabilities: {}",
                        format_capabilities(&agent.capabilities)
                    );
                }
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
        ConfigAction::PromptBudget => {
            let skills = skills::list_skills()?;
            if skills.is_empty() {
                println!("No skills found in ~/.aid/skills/.");
                println!("  Run `aid init` to install default skills.");
            } else {
                println!("Skill Token Budget:");
                let mut total_tokens = 0usize;
                for skill in &skills {
                    let (_, tokens) = skills::measure_skill_tokens(skill)?;
                    total_tokens += tokens;
                    println!("  {:14} ~{} tokens", skill, tokens);
                }
                println!("  ─────────────────────");
                println!("  Total:         ~{} tokens", total_tokens);
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
        ConfigAction::Pricing { update } => {
            if update {
                let updated = update_pricing_file()?;
                println!("Updated {updated} models in {}.", crate::paths::pricing_path().display());
            }
            let pricing = merged_agent_models()?;
            println!(
                "{:<10} {:<25} {:>10} {:>10} {:>10} Description",
                "Agent", "Model", "Tier", "Input/M", "Output/M"
            );
            println!("{}", "-".repeat(85));
            for agent in [
                AgentKind::Codex,
                AgentKind::Gemini,
                AgentKind::OpenCode,
                AgentKind::Kilo,
                AgentKind::Cursor,
            ] {
                for am in pricing.iter().filter(|m| m.agent == agent) {
                    println!(
                        "{:<10} {:<25} {:>10} ${:>9.2} ${:>9.2} {}",
                        agent.as_str(),
                        am.model,
                        am.tier,
                        am.input_per_m,
                        am.output_per_m,
                        am.description
                    );
                }
            }
        }
        ConfigAction::ClearLimit { agent } => {
            if agent == "all" {
                for (kind, _, _, _, _) in AGENT_PROFILES {
                    if rate_limit::clear_rate_limit(kind) {
                        println!("Cleared rate-limit for {}", kind.as_str());
                    }
                }
            } else {
                let Some(kind) = AgentKind::parse_str(&agent) else {
                    anyhow::bail!("Unknown agent: {agent}");
                };
                if rate_limit::clear_rate_limit(&kind) {
                    println!("Cleared rate-limit for {}", kind.as_str());
                } else {
                    println!("{} is not rate-limited", agent);
                }
            }
        }
        ConfigAction::AddAgent { .. } => {
            println!("Custom agent registration not yet implemented");
        }
    }
    Ok(())
}

pub fn load_pricing_overrides() -> Result<Vec<PricingFileModel>> {
    let path = crate::paths::pricing_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    let response: PricingResponse = serde_json::from_str(&contents)?;
    Ok(response.models)
}

pub fn merged_agent_models() -> Result<Vec<ResolvedAgentModel>> {
    let mut merged = Vec::with_capacity(AGENT_MODELS.len());
    let mut indexes = HashMap::new();
    for model in AGENT_MODELS {
        indexes.insert((model.agent, model.model.to_lowercase()), merged.len());
        merged.push(ResolvedAgentModel {
            agent: model.agent,
            model: model.model.to_string(),
            input_per_m: model.input_per_m,
            output_per_m: model.output_per_m,
            tier: model.tier.to_string(),
            description: model.description.to_string(),
        });
    }
    for model in load_pricing_overrides()? {
        let PricingFileModel {
            agent: agent_name,
            model,
            input_per_m,
            output_per_m,
            tier,
            description,
            updated,
        } = model;
        let _ = updated;
        let Some(agent) = AgentKind::parse_str(&agent_name) else {
            continue;
        };
        let key = (agent, model.to_lowercase());
        if let Some(index) = indexes.get(&key).copied() {
            let current = &mut merged[index];
            current.input_per_m = input_per_m;
            current.output_per_m = output_per_m;
            current.tier = tier;
            current.description = description;
        } else {
            indexes.insert(key, merged.len());
            merged.push(ResolvedAgentModel {
                agent,
                model,
                input_per_m,
                output_per_m,
                tier,
                description,
            });
        }
    }
    Ok(merged)
}

fn update_pricing_file() -> Result<usize> {
    let output = Command::new("curl")
        .args(["-fsSL", "https://aid.agent-tools.org/api/pricing"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("curl failed with status {}", output.status);
    }
    let body = String::from_utf8(output.stdout)?;
    let response: PricingResponse = serde_json::from_str(&body)?;
    let path = crate::paths::pricing_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body)?;
    Ok(response.models.len())
}

fn command_installed(command: &str) -> bool {
    let binary = command.split_whitespace().next().unwrap_or_default();
    if binary.is_empty() {
        return false;
    }
    Command::new("which")
        .arg(binary)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn format_capabilities(cap: &CapabilityScores) -> String {
    [
        ("research", cap.research),
        ("simple_edit", cap.simple_edit),
        ("complex_impl", cap.complex_impl),
        ("frontend", cap.frontend),
        ("debugging", cap.debugging),
        ("testing", cap.testing),
        ("refactoring", cap.refactoring),
        ("documentation", cap.documentation),
    ]
    .into_iter()
    .map(|(label, value)| format!("{label}={value}"))
    .collect::<Vec<_>>()
    .join(", ")
}

fn agent_profile(
    kind: AgentKind,
    installed: bool,
    history: Option<&AgentHistory>,
    model_history: &HashMap<(AgentKind, String), ModelHistory>,
) -> String {
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
    let models_line = {
        let agent_models: Vec<_> = AGENT_MODELS.iter().filter(|m| m.agent == kind).collect();
        if agent_models.is_empty() {
            "  Models:    none configured\n".to_string()
        } else {
            let mut lines = "  Models:\n".to_string();
            for am in &agent_models {
                let history_key = (kind, am.model.to_string());
                let history_suffix = match model_history.get(&history_key) {
                    Some(h) => format!(
                        "  [{} tasks, {:.0}% success, {}/task]",
                        h.task_count,
                        h.success_rate,
                        cost::format_cost(Some(h.avg_cost))
                    ),
                    None => String::new(),
                };
                lines.push_str(&format!(
                    "    {:<15} ({}, cap:{:.1}, ${:.2}/${:.2}/M)  {}{}\n",
                    am.model, am.tier, am.capability, am.input_per_m, am.output_per_m, am.description, history_suffix
                ));
            }
            lines
        }
    };
    let rate_limit_line = match rate_limit::get_rate_limit_info(&kind) {
        Some(info) if info.recovery_at.is_some() => {
            let fallback_hint = crate::agent::selection::coding_fallback_for(&kind)
                .map(|fb| format!(" → use --fallback {}", fb.as_str()))
                .unwrap_or_default();
            format!(
                "  Status:    rate-limited (try again at {}){}\n",
                info.recovery_at.as_deref().unwrap_or("unknown"),
                fallback_hint,
            )
        }
        _ => "".to_string(),
    };
    format!(
        "  Strengths: {}\n  Cost:      {}\n{}{}{}  Mode:      {} ({})\n",
        strengths, cost, history_line, rate_limit_line, models_line, mode, install_status
    )
}

struct AgentHistory {
    task_count: usize,
    success_rate: f64,
    avg_cost: f64,
}

struct ModelHistory {
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

fn compute_model_history(
    tasks: &[crate::types::Task],
) -> HashMap<(AgentKind, String), ModelHistory> {
    let mut accum: HashMap<(AgentKind, String), (usize, usize, f64)> = HashMap::new();
    for task in tasks {
        let model = task.model.clone().unwrap_or_else(|| "default".to_string());
        let entry = accum.entry((task.agent, model)).or_insert((0, 0, 0.0));
        entry.0 += 1;
        if matches!(task.status, TaskStatus::Done | TaskStatus::Merged) {
            entry.1 += 1;
        }
        if let Some(c) = task.cost_usd {
            entry.2 += c;
        }
    }
    accum
        .into_iter()
        .map(|(key, (count, done, total_cost))| {
            (
                key,
                ModelHistory {
                    task_count: count,
                    success_rate: (done as f64 / count as f64) * 100.0,
                    avg_cost: total_cost / count as f64,
                },
            )
        })
        .collect()
}

pub fn models_for_agent(agent: &AgentKind) -> Vec<&'static AgentModel> {
    AGENT_MODELS.iter().filter(|m| m.agent == *agent).collect()
}

pub fn budget_model(agent: &AgentKind) -> Option<&'static str> {
    let models = models_for_agent(agent);
    if models.is_empty() {
        return None;
    }
    let non_free: Vec<_> = models.iter().filter(|m| m.tier != "free").collect();
    if non_free.is_empty() {
        models.first().map(|m| m.model)
    } else {
        non_free
            .iter()
            .min_by(|a, b| {
                let cost_a = a.input_per_m + a.output_per_m;
                let cost_b = b.input_per_m + b.output_per_m;
                cost_a.partial_cmp(&cost_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|m| m.model)
    }
}

#[cfg(test)]
mod tests {
    use super::{load_pricing_overrides, merged_agent_models};
    use crate::paths::AidHomeGuard;

    #[test]
    fn loads_and_merges_pricing_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        std::fs::write(
            crate::paths::pricing_path(),
            r#"{
                "models": [
                    {
                        "agent": "codex",
                        "model": "gpt-4.1",
                        "input_per_m": 9.0,
                        "output_per_m": 19.0,
                        "tier": "custom",
                        "description": "override",
                        "updated": "2026-03-17"
                    },
                    {
                        "agent": "codex",
                        "model": "new-model",
                        "input_per_m": 1.5,
                        "output_per_m": 2.5,
                        "tier": "cheap",
                        "description": "new entry",
                        "updated": "2026-03-17"
                    }
                ]
            }"#,
        )
        .unwrap();

        let loaded = load_pricing_overrides().unwrap();
        assert_eq!(loaded.len(), 2);

        let merged = merged_agent_models().unwrap();
        let existing = merged
            .iter()
            .find(|model| model.agent == crate::types::AgentKind::Codex && model.model == "gpt-4.1")
            .unwrap();
        assert_eq!(existing.input_per_m, 9.0);
        assert_eq!(existing.output_per_m, 19.0);
        assert_eq!(existing.tier, "custom");
        let added = merged
            .iter()
            .find(|model| model.agent == crate::types::AgentKind::Codex && model.model == "new-model")
            .unwrap();
        assert_eq!(added.input_per_m, 1.5);
        assert_eq!(added.output_per_m, 2.5);
    }
}

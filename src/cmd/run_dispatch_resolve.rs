// Agent and project resolution helpers for `aid run` dispatch setup.
// Exports: AgentSetup, apply_project_defaults(), resolve_agent_setup().
// Deps: agent registry, config, project defaults, budget/rate-limit helpers.
use anyhow::Result;
use std::sync::Arc;
use crate::agent;
use crate::agent_config;
use crate::cmd::config as cmd_config;
use crate::config;
use crate::project::ProjectConfig;
use crate::rate_limit;
use crate::store::Store;
use crate::types::AgentKind;
use crate::usage;
use super::run_prompt;
use super::RunArgs;

/// Emit the one-time GitButler setup hint as a task milestone event.
pub(super) fn insert_gitbutler_setup_hint(store: &Store, task_id: &crate::types::TaskId) {
    let _ = store.insert_event(&crate::types::TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: crate::types::EventKind::Milestone,
        detail: "Hint: run `but setup` from the main repo to enable GitButler integration for future tasks."
            .to_string(),
        metadata: None,
    });
}

pub(super) struct AgentSetup {
    pub agent_kind: AgentKind,
    pub custom_agent_name: Option<String>,
    pub agent_display_name: String,
    pub requested_skills: Vec<String>,
    pub effective_model: Option<String>,
    pub budget_active: bool,
    pub agent: Box<dyn agent::Agent>,
    /// `Some((original, hold))` when a held primary was replaced before dispatch.
    pub substituted_from: Option<(String, String)>,
}

pub(super) fn apply_project_defaults(args: &mut RunArgs, detected_project: Option<&ProjectConfig>) {
    if let Some(project) = detected_project {
        let mut defaults_applied = false;
        if args.max_task_cost.is_none() {
            args.max_task_cost = project.max_task_cost;
        }
        if args.team.is_none()
            && let Some(team) = project.team.as_ref() {
                args.team = Some(team.clone());
                defaults_applied = true;
            }
        if args.verify.is_none()
            && let Some(verify) = project.verify.as_ref() {
                args.verify = Some(verify.clone());
                defaults_applied = true;
            }
        if args.setup.is_none()
            && let Some(setup) = project.setup.as_ref() {
                args.setup = Some(setup.clone());
                defaults_applied = true;
            }
        if args.container.is_none()
            && let Some(container) = project.container.as_ref() {
                args.container = Some(container.clone());
                defaults_applied = true;
            }
        if !args.budget && project.budget.prefer_budget {
            args.budget = true;
            defaults_applied = true;
        }
        if args.no_audit {
            args.audit = false;
        } else if !args.audit_explicit && project.audit_auto() {
            args.audit = true;
            defaults_applied = true;
        }
        if defaults_applied {
            aid_info!(
                "[aid] Project '{}' defaults: team={}, verify={}, audit={}",
                project.id,
                args.team.as_deref().unwrap_or("None"),
                args.verify.as_deref().unwrap_or("None"),
                if args.audit { "on" } else { "off" },
            );
        }
    }
}

pub(super) fn resolve_agent_setup(store: &Arc<Store>, args: &mut RunArgs) -> Result<AgentSetup> {
    let (mut agent_kind, mut custom_agent_name) = if let Some(kind) = AgentKind::parse_str(&args.agent_name) {
        (kind, None)
    } else if agent::registry::custom_agent_exists(&args.agent_name) {
        (AgentKind::Custom, Some(args.agent_name.clone()))
    } else {
        let custom = agent::registry::list_custom_agents();
        let mut available = AgentKind::ALL_BUILTIN
            .iter()
            .map(AgentKind::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        for ca in &custom {
            available.push_str(&format!(", {}", ca.id));
        }
        anyhow::bail!("Unknown agent '{}'. Available: {}", args.agent_name, available);
    };
    let resolved_agent_name = custom_agent_name
        .as_deref()
        .unwrap_or_else(|| agent_kind.as_str());
    if agent_config::is_agent_disabled(resolved_agent_name) {
        anyhow::bail!(
            "Agent '{resolved_agent_name}' is disabled (enable with: aid agent config {resolved_agent_name} --enable)"
        );
    }
    if args.dir.is_none()
        && args.worktree.is_none()
        && matches!(
            agent_kind,
            AgentKind::Codex
                | AgentKind::Copilot
                | AgentKind::Claude
                | AgentKind::OpenCode
                | AgentKind::Cursor
                | AgentKind::Kilo
                | AgentKind::MiMoCode
                | AgentKind::Droid
                | AgentKind::Grok
                | AgentKind::Custom
        )
        && std::path::Path::new(".git").exists()
    {
        args.dir = Some(".".to_string());
        aid_info!("[aid] Auto-set --dir . (git repo detected)");
    }
    let mut substituted_from: Option<(String, String)> = None;
    if args.declared_urgency == Some(crate::types::TaskUrgency::Background)
        && rate_limit::is_rate_limited(&agent_kind)
    {
        aid_warn!(
            "[aid] {} is rate-limited; background urgency keeps this agent selected",
            agent_kind.as_str()
        );
    } else if let Some(hold) = rate_limit::dispatch_blocking_hold(&agent_kind) {
        let original = agent_kind.as_str().to_string();
        let (next_kind, remaining) = skip_held_to_fallback(agent_kind, &hold, &args.cascade, &args.prompt)?;
        aid_warn!(
            "[aid] {} is held ({}) — dispatching to {} instead. \
             Use `aid config clear-limit {}` to clear.",
            original, hold, next_kind.as_str(), original,
        );
        super::switch_agent(args, next_kind.as_str().to_string());
        args.cascade = remaining;
        agent_kind = next_kind;
        custom_agent_name = None;
        substituted_from = Some((original, hold));
    }
    let requested_skills = run_prompt::effective_skills(args);
    if args.skills.is_empty() {
        for skill in &requested_skills {
            aid_info!("[aid] Auto-applied skill: {skill}");
        }
    }
    let cfg = config::load_config()?;
    let budget_status = usage::check_budget_status(store, &cfg)?;
    if budget_status.over_limit {
        if let Some(msg) = budget_status.message {
            anyhow::bail!("Budget limit exceeded:\n{msg}");
        } else {
            anyhow::bail!("Budget limit exceeded");
        }
    }
    let auto_budget = if budget_status.near_limit && !cfg.selection.budget_mode {
        if let Some(ref msg) = budget_status.message {
            aid_warn!("[aid] Warning: {}\n[aid] Auto-enabling budget mode", msg);
        }
        true
    } else {
        false
    };
    // Self-heal retries (force_default_model) bypass model selection entirely so
    // the agent runs on its own current default — the only always-valid choice
    // after a "model unavailable" failure.
    let requested_model = if args.force_default_model {
        None
    } else {
        args.model.clone().or_else(|| agent_config::get_default_model(&args.agent_name))
    };
    let budget_active =
        !args.force_default_model && (args.budget || auto_budget || cfg.selection.budget_mode);
    let smart_routed = if !args.force_default_model
        && !budget_active
        && requested_model.is_none()
        && cfg.selection.smart_routing
        && crate::agent::classifier::is_simple_for_routing(&args.prompt)
    {
        if let Some(bm) = cmd_config::budget_model(&agent_kind) {
            if rate_limit::is_rate_limited(&agent_kind) {
                None
            } else {
                aid_info!("[aid] Smart route: simple prompt -> {}", bm);
                Some(bm.to_string())
            }
        } else {
            None
        }
    } else {
        None
    };
    let effective_model = smart_routed.or_else(|| {
        if budget_active && requested_model.is_none() {
            if let Some(bm) = cmd_config::budget_model(&agent_kind) {
                aid_info!("[aid] Budget mode: using model {}", bm);
                Some(bm.to_string())
            } else {
                requested_model.clone()
            }
        } else {
            requested_model.clone()
        }
    });
    // An agent whose plan meters model families separately can have one family
    // exhausted while another still serves. Switch groups rather than treating
    // the agent as unavailable — and say so, because a silent model swap is the
    // same defect as a CLI quietly substituting a model the caller did not ask
    // for.
    let effective_model = match agent::model_group::healthy_model_for(
        agent_kind,
        effective_model.as_deref(),
        |group| rate_limit::is_group_rate_limited(&agent_kind, group),
    ) {
        Some(replacement) => {
            aid_warn!(
                "[aid] {} model group exhausted; switching {} -> {}",
                agent_kind.as_str(),
                effective_model.as_deref().unwrap_or("(default)"),
                replacement
            );
            Some(replacement.to_string())
        }
        None => effective_model,
    };
    let agent: Box<dyn agent::Agent> = if agent_kind == AgentKind::Custom {
        agent::registry::resolve_custom_agent(custom_agent_name.as_deref().unwrap_or(""))
            .ok_or_else(|| anyhow::anyhow!("Custom agent '{}' not found in registry", args.agent_name))?
    } else {
        agent::get_agent(agent_kind)
    };
    if let Some(ref model) = effective_model {
        agent::model_validation::validate_model_for_agent(agent.as_ref(), model)?;
    }
    Ok(AgentSetup {
        agent_kind,
        custom_agent_name: custom_agent_name.clone(),
        agent_display_name: custom_agent_name
            .as_deref()
            .unwrap_or_else(|| agent_kind.as_str())
            .to_string(),
        requested_skills,
        effective_model,
        budget_active,
        agent,
        substituted_from,
    })
}

// Walk `cascade` (then auto-fallback) to the first non-held alternative.
fn skip_held_to_fallback(held: AgentKind, hold: &str, cascade: &[String], prompt: &str) -> Result<(AgentKind, Vec<String>)> {
    let all: Vec<AgentKind> = cascade.iter().filter_map(|s| AgentKind::parse_str(s)).collect();
    let start = all.iter().position(|k| *k == held).map_or(0, |i| i + 1);
    for (i, kind) in all[start..].iter().enumerate() {
        if rate_limit::dispatch_blocking_hold(kind).is_none() {
            return Ok((*kind, all[start + i + 1..].iter().map(|k| k.as_str().to_string()).collect()));
        }
    }
    if let Some(fb) = crate::agent::selection::coding_fallback_for_prompt(&held, prompt)
        && rate_limit::dispatch_blocking_hold(&fb).is_none() {
        return Ok((fb, vec![]));
    }
    anyhow::bail!("{} is held ({hold}). Use --cascade <agent> to specify a fallback, or `aid config clear-limit {}` to clear.", held.as_str(), held.as_str())
}
// Insert a milestone event when a held route was substituted. No-op if `substituted_from` is None.
pub(super) fn maybe_insert_held_route_event(store: &Store, task_id: &crate::types::TaskId, setup: &AgentSetup) {
    let Some((ref original, ref hold)) = setup.substituted_from else { return };
    let _ = store.insert_event(&crate::types::TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: crate::types::EventKind::Milestone,
        detail: format!(
            "Held route skipped: {original} ({hold}) — dispatching to {} instead. \
             Use `aid config clear-limit {original}` to restore.",
            setup.agent_display_name,
        ),
        metadata: None,
    });
}

#[cfg(test)]
#[path = "run_dispatch_resolve_tests.rs"]
mod tests;

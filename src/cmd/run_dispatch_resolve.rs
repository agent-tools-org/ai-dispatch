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

pub(super) struct AgentSetup {
    pub agent_kind: AgentKind,
    pub custom_agent_name: Option<String>,
    pub agent_display_name: String,
    pub requested_skills: Vec<String>,
    pub effective_model: Option<String>,
    pub budget_active: bool,
    pub agent: Box<dyn agent::Agent>,
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
        if args.container.is_none()
            && let Some(container) = project.container.as_ref() {
                args.container = Some(container.clone());
                defaults_applied = true;
            }
        if !args.budget && project.budget.prefer_budget {
            args.budget = true;
            defaults_applied = true;
        }
        if defaults_applied {
            aid_info!(
                "[aid] Project '{}' defaults: team={}, verify={}",
                project.id,
                args.team.as_deref().unwrap_or("None"),
                args.verify.as_deref().unwrap_or("None"),
            );
        }
    }
}

pub(super) fn resolve_agent_setup(store: &Arc<Store>, args: &mut RunArgs) -> Result<AgentSetup> {
    let (agent_kind, custom_agent_name) = if let Some(kind) = AgentKind::parse_str(&args.agent_name) {
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
                | AgentKind::Codebuff
                | AgentKind::Droid
                | AgentKind::Custom
        )
        && std::path::Path::new(".git").exists()
    {
        args.dir = Some(".".to_string());
        aid_info!("[aid] Auto-set --dir . (git repo detected)");
    }
    if let Some(info) = rate_limit::get_rate_limit_info(&agent_kind)
        && let Some(ref recovery) = info.recovery_at
    {
        if let Some(next_agent) = args.cascade.first() {
            aid_warn!(
                "[aid] {} is rate-limited — will cascade to {}",
                agent_kind.as_str(),
                next_agent
            );
        } else if let Some(fallback) = crate::agent::selection::coding_fallback_for(&agent_kind) {
            aid_warn!(
                "[aid] {} is rate-limited (until {}), auto-cascading to {}",
                agent_kind.as_str(),
                recovery,
                fallback.as_str()
            );
            args.cascade = vec![fallback.as_str().to_string()];
        } else {
            anyhow::bail!(
                "{} is rate-limited until {}. Use --cascade <agent> to specify a fallback, or wait.",
                agent_kind.as_str(),
                recovery
            );
        }
    }
    let requested_skills = run_prompt::effective_skills(&agent_kind, args);
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
    let requested_model =
        args.model.clone().or_else(|| agent_config::get_default_model(&args.agent_name));
    let budget_active = args.budget || auto_budget || cfg.selection.budget_mode;
    let smart_routed = if !budget_active
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
    let agent: Box<dyn agent::Agent> = if agent_kind == AgentKind::Custom {
        agent::registry::resolve_custom_agent(custom_agent_name.as_deref().unwrap_or(""))
            .ok_or_else(|| anyhow::anyhow!("Custom agent '{}' not found in registry", args.agent_name))?
    } else {
        agent::get_agent(agent_kind)
    };
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
    })
}

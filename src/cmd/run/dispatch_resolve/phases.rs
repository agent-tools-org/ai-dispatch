// Agent identity, budget, hold, and model resolution phases.
// Deps: parent module types and helpers.
use super::*;

fn resolve_agent_identity(
    args: &RunArgs,
    has_requested_api: bool,
) -> Result<(AgentKind, Option<String>)> {
    let (agent_kind, custom_agent_name) = if let Some(kind) = AgentKind::parse_str(&args.agent_name)
    {
        (kind, None)
    } else if has_requested_api || agent::registry::custom_agent_exists(&args.agent_name) {
        (AgentKind::Custom, Some(args.agent_name.clone()))
    } else {
        let custom = agent::registry::list_custom_agents();
        let mut available = agent::routable_builtins()
            .map(|kind| kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        for ca in &custom {
            available.push_str(&format!(", {}", ca.id));
        }
        anyhow::bail!(
            "Unknown agent '{}'. Available: {}",
            args.agent_name,
            available
        );
    };
    Ok((agent_kind, custom_agent_name))
}

pub(super) fn set_git_working_directory(args: &mut RunArgs, agent_kind: AgentKind) {
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
}

pub(super) fn resolve_auto_budget(
    store: &Store,
    cfg: &config::AidConfig,
    project_id: Option<&str>,
) -> Result<bool> {
    let budget_status = usage::check_budget_status_for_project(store, cfg, project_id)?;
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
    Ok(auto_budget)
}

pub(super) fn resolve_healthy_model(
    args: &mut RunArgs,
    agent_kind: AgentKind,
    custom_agent_name: &Option<String>,
    effective_model: Option<String>,
) -> Option<String> {
    if args.model_source == agent::model_validation::ModelSource::Advised {
        effective_model
    } else {
        match agent::model_group::healthy_model_for(
            agent_kind,
            effective_model.as_deref(),
            |group| {
                rate_limit::is_group_rate_limited(&agent_kind, custom_agent_name.as_deref(), group)
            },
        ) {
            Some(replacement) => {
                aid_warn!(
                    "[aid] {} model group exhausted; switching {} -> {}",
                    agent_kind.as_str(),
                    effective_model.as_deref().unwrap_or("(default)"),
                    replacement
                );
                args.model_source = agent::model_validation::ModelSource::AidResolved;
                Some(replacement.to_string())
            }
            None => effective_model,
        }
    }
}

fn resolve_agent_instance(
    args: &RunArgs,
    agent_kind: AgentKind,
    custom_agent_name: &Option<String>,
) -> Result<Box<dyn agent::Agent>> {
    let agent: Box<dyn agent::Agent> = if agent_kind == AgentKind::Custom {
        agent::registry::resolve_custom_agent(custom_agent_name.as_deref().unwrap_or(""))
            .ok_or_else(|| {
                anyhow::anyhow!("Custom agent '{}' not found in registry", args.agent_name)
            })?
    } else if agent_kind == AgentKind::Codex {
        agent::codex::routed(custom_agent_name.clone())
    } else {
        agent::get_agent(agent_kind)
    };
    Ok(agent)
}

pub(super) fn resolve_held_route(
    store: &Store,
    args: &mut RunArgs,
    mut agent_kind: AgentKind,
    mut custom_agent_name: Option<String>,
    resolved_agent_name: &str,
) -> Result<(AgentKind, Option<String>, Option<(String, String)>)> {
    let api_hold = crate::config::api_route::find_for_dispatch(resolved_agent_name)?
        .map(|route| crate::routing::api_budget::for_route(&route, Some(store)))
        .transpose()?
        .filter(|budget| {
            budget.billing.state == crate::routing::quota::billing::BillingState::Exhausted
        })
        .map(|budget| budget.label());
    let mut substituted_from: Option<(String, String)> = None;
    if args.declared_urgency == Some(crate::types::TaskUrgency::Background)
        && crate::config::api_route::find_for_dispatch(resolved_agent_name)?.is_none()
        && held::background_keeps_hold(agent_kind, custom_agent_name.as_deref())
    {
        aid_warn!(
            "[aid] {} is rate-limited; background urgency keeps this agent selected",
            resolved_agent_name
        );
    } else if let Some(hold) = api_hold
        .or_else(|| rate_limit::dispatch_blocking_hold(&agent_kind, custom_agent_name.as_deref()))
    {
        let original = custom_agent_name
            .as_deref()
            .unwrap_or_else(|| agent_kind.as_str())
            .to_string();
        let (next_kind, next_name) = held::skip_held_to_fallback(args, &original, &hold)?;
        aid_warn!(
            "[aid] {} is held ({}) — dispatching to {} instead. Use `aid config clear-limit {}` to clear.",
            original,
            hold,
            next_name,
            original
        );
        agent_kind = next_kind;
        custom_agent_name = route_custom_name(next_kind, &next_name)?;
        substituted_from = Some((original, hold));
    }
    held::warn_if_degraded(agent_kind, custom_agent_name.as_deref());
    Ok((agent_kind, custom_agent_name, substituted_from))
}

pub(super) fn resolve_model_hold(
    args: &mut RunArgs,
    mut agent_kind: AgentKind,
    mut custom_agent_name: Option<String>,
    mut effective_model: Option<String>,
    mut substituted_from: Option<(String, String)>,
) -> Result<(
    AgentKind,
    Option<String>,
    Option<String>,
    Option<(String, String)>,
)> {
    // No-group for_model is agent-level; skip it or background keep is undone.
    if (agent::model_group::model_group(agent_kind, effective_model.as_deref()).is_some()
        || !crate::routing::capacity::notes(&agent_kind, custom_agent_name.as_deref()).is_empty())
        && let Some(hold) = rate_limit::dispatch_blocking_hold_for_model(
            &agent_kind,
            custom_agent_name.as_deref(),
            effective_model.as_deref(),
        )
    {
        if crate::routing::availability::availability_for_group(
            &agent_kind,
            custom_agent_name.as_deref(),
            &crate::routing::capacity::group(effective_model.as_deref()),
        )
        .status
            == crate::routing::availability::RouteStatus::Held
        {
            anyhow::bail!("{} model is held ({hold})", args.agent_name);
        }
        held::switch_model_held_route(
            args,
            &mut agent_kind,
            &mut custom_agent_name,
            &mut effective_model,
            &mut substituted_from,
            hold,
        )?;
    }
    Ok((
        agent_kind,
        custom_agent_name,
        effective_model,
        substituted_from,
    ))
}

pub(super) fn finish_agent_setup(
    args: &mut RunArgs,
    agent_kind: AgentKind,
    custom_agent_name: Option<String>,
    mut effective_model: Option<String>,
    substituted_from: Option<(String, String)>,
    budget_active: bool,
    run_model: crate::agent::run_model::RunModel,
) -> Result<AgentSetup> {
    let agent = resolve_agent_instance(args, agent_kind, &custom_agent_name)?;
    agent::api_route::reject_container(agent.as_ref(), args.container.is_some())?;
    let model_source = if args.model.is_some()
        || args.model_source == agent::model_validation::ModelSource::Advised
    {
        args.model_source
    } else {
        agent::model_validation::ModelSource::AidResolved
    };
    args.model_source = model_source;
    if let Some(ref model) = effective_model
        && !held::keep_aid_resolved_pin(substituted_from.as_ref(), model_source)
        && !agent::model_validation::validate_model_for_agent(agent.as_ref(), model, model_source)?
    {
        effective_model = None;
    }
    aid_info!(
        "{}",
        model_info::model_selection_info(
            args,
            &run_model,
            effective_model.as_deref(),
            agent.as_ref()
        )
    );
    Ok(AgentSetup {
        agent_kind,
        custom_agent_name: custom_agent_name.clone(),
        agent_display_name: custom_agent_name
            .as_deref()
            .filter(|_| agent_kind == AgentKind::Custom)
            .unwrap_or_else(|| agent_kind.as_str())
            .to_string(),
        effective_model,
        budget_active: budget_active && !args.force_default_model,
        agent,
        substituted_from,
    })
}

pub(super) fn resolve_enabled_agent(args: &RunArgs) -> Result<(AgentKind, Option<String>, String)> {
    let requested_api = config::api_route::find_for_dispatch(&args.agent_name)?;
    let (agent_kind, mut custom_agent_name) =
        resolve_agent_identity(args, requested_api.is_some())?;
    let resolved_agent_name = custom_agent_name
        .as_deref()
        .unwrap_or_else(|| agent_kind.as_str())
        .to_string();
    if agent_config::is_agent_disabled(&resolved_agent_name) {
        anyhow::bail!(
            "Agent '{resolved_agent_name}' is disabled (enable with: aid agent config \
            {resolved_agent_name} --enable)"
        );
    }
    custom_agent_name = route_custom_name(agent_kind, &args.agent_name)?;
    Ok((agent_kind, custom_agent_name, resolved_agent_name))
}

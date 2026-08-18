// Held-route cascade walking for pre-dispatch agent substitution.
// Exports: skip_held_to_fallback(), maybe_insert_held_route_event().
// Deps: agent registry/selection, rate_limit, Store, AgentSetup.
use anyhow::Result;
use crate::agent;
use crate::rate_limit;
use crate::store::Store;
use crate::types::AgentKind;
use super::AgentSetup;

pub(super) fn switch_model_held_route(
    args: &mut super::RunArgs,
    agent_kind: &mut AgentKind,
    custom_agent_name: &mut Option<String>,
    effective_model: &mut Option<String>,
    substituted_from: &mut Option<(String, String)>,
    hold: String,
) -> Result<()> {
    let original = custom_agent_name
        .as_deref()
        .unwrap_or_else(|| agent_kind.as_str())
        .to_string();
    let (next_kind, next_name, remaining) =
        skip_held_to_fallback(*agent_kind, &original, &hold, &args.cascade, &args.prompt)?;
    aid_warn!(
        "[aid] {} model provider is held ({}) — dispatching to {} instead. Use `aid config clear-limit {}` to clear.",
        original, hold, next_name, original
    );
    super::super::switch_agent(args, next_name.clone());
    args.cascade = remaining;
    *agent_kind = next_kind;
    *custom_agent_name = (next_kind == AgentKind::Custom).then_some(next_name);
    *substituted_from = Some((original, hold));
    let next_custom = custom_agent_name.as_deref();
    // `None` would run the fallback's own default model — for agy that is a
    // gemini one, the exact family the hold just proved exhausted (t-44b30780).
    // The old model belongs to the substituted CLI, so evaluate the next agent
    // with no current model and pin a healthy group when it has one (agy on its
    // claude family); ungrouped agents keep their own default.
    *effective_model = agent::model_group::healthy_model_for(
        *agent_kind,
        None,
        |group| rate_limit::is_group_rate_limited(agent_kind, next_custom, group),
    )
    .map(str::to_string);
    Ok(())
}

// Walk `cascade` (then auto-fallback) to the first non-held alternative. Unrecognised names error; custom agents are valid.
pub(super) fn skip_held_to_fallback(
    held_kind: AgentKind,
    held_name: &str,
    hold: &str,
    cascade: &[String],
    prompt: &str,
) -> Result<(AgentKind, String, Vec<String>)> {
    let all: Vec<(AgentKind, String)> = cascade
        .iter()
        .map(|s| {
            AgentKind::parse_str(s)
                .map(|k| (k, s.clone()))
                .or_else(|| {
                    agent::registry::custom_agent_exists(s).then(|| (AgentKind::Custom, s.clone()))
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unknown cascade agent '{s}'. Use `aid config agents` to list available agents."
                    )
                })
        })
        .collect::<Result<_>>()?;
    let start = all
        .iter()
        .position(|(_, n)| n == held_name)
        .map_or(0, |i| i + 1);
    for (i, (kind, name)) in all[start..].iter().enumerate() {
        // A custom candidate is held under its own name, not the shared
        // `custom` marker — see rate_limit::marker_path.
        let candidate_custom = (*kind == AgentKind::Custom).then_some(name.as_str());
        if !candidate_is_blocked(kind, candidate_custom) {
            return Ok((
                *kind,
                name.clone(),
                all[start + i + 1..]
                    .iter()
                    .map(|(_, n)| n.clone())
                    .collect(),
            ));
        }
    }
    if let Some(fb) = crate::agent::selection::coding_fallback_for_prompt(&held_kind, prompt)
        .filter(|fb| !candidate_is_blocked(fb, None))
    {
        return Ok((fb, fb.as_str().to_string(), vec![]));
    }
    anyhow::bail!(
        "{held_name} is held ({hold}). Use --cascade <agent> or `aid config clear-limit {held_name}` to clear."
    )
}

/// Whether a fallback candidate can actually take the work. An agent-level
/// hold blocks outright; a candidate with a static family table (agy, cursor)
/// is blocked when every group it can draw on is held — the agent-level
/// marker alone would miss `rate-limit-agy--gemini`, so a substitution would
/// hand the task to an allowance that is already spent (t-44b30780). opencode
/// keeps its dynamic provider markers; its candidate check stays agent-level.
fn candidate_is_blocked(kind: &AgentKind, custom_name: Option<&str>) -> bool {
    if rate_limit::dispatch_blocking_hold(kind, custom_name).is_some() {
        return true;
    }
    let groups = crate::agent::model_group::groups_for_agent(*kind);
    if groups.is_empty() {
        return false;
    }
    groups.iter().all(|(group, _)| {
        rate_limit::is_group_rate_limited(kind, custom_name, group)
    })
}

fn wall_label(wall: crate::route_availability::QuotaWall) -> &'static str {
    match wall {
        crate::route_availability::QuotaWall::Clock => "clock",
        crate::route_availability::QuotaWall::Windowed => "windowed",
        crate::route_availability::QuotaWall::Prepaid => "prepaid",
        crate::route_availability::QuotaWall::PlanChange => "plan_change",
        crate::route_availability::QuotaWall::Transient => "transient",
        crate::route_availability::QuotaWall::None => "none",
    }
}

fn model_tier(kind: AgentKind, model: Option<&str>) -> Option<String> {
    let model = model?;
    crate::model_catalog::models_for_agent(&kind)
        .into_iter()
        .find(|entry| entry.model == model)
        .map(|entry| entry.tier)
}

pub(crate) fn model_class_preserved(
    from_agent: &str,
    to_agent: &str,
    from_model: Option<&str>,
    to_model: Option<&str>,
) -> bool {
    let Some(from_kind) = AgentKind::parse_str(from_agent) else {
        return false;
    };
    let Some(to_kind) = AgentKind::parse_str(to_agent) else {
        return false;
    };
    match (
        model_tier(from_kind, from_model),
        model_tier(to_kind, to_model),
    ) {
        (Some(from_tier), Some(to_tier)) => from_tier == to_tier,
        _ => false,
    }
}

pub(crate) fn held_substitution_detail(
    original: &str,
    hold: &str,
    to: &str,
    dry_run: bool,
) -> String {
    let verb = if dry_run {
        "would dispatch"
    } else {
        "dispatching"
    };
    format!(
        "Held route skipped: {original} ({hold}) — {verb} to {to} instead. \
         Use `aid config clear-limit {original}` to restore."
    )
}

pub(crate) fn held_substitution_metadata(
    original: &str,
    to: &str,
    from_model: Option<&str>,
    to_model: Option<&str>,
    hold: &str,
    wall: &str,
    dry_run: bool,
) -> serde_json::Value {
    serde_json::json!({
        "kind": "quota_substitution",
        "from_agent": original,
        "to_agent": to,
        "from_model": from_model,
        "to_model": to_model,
        "wall": wall,
        "hold": hold,
        "model_class_preserved": model_class_preserved(original, to, from_model, to_model),
        "dry_run": dry_run,
    })
}

// Insert a milestone event when a held route was substituted. No-op if `substituted_from` is None.
pub(crate) fn maybe_insert_held_route_event(
    store: &Store,
    task_id: &crate::types::TaskId,
    setup: &AgentSetup,
    dry_run: bool,
) {
    let Some((ref original, ref hold)) = setup.substituted_from else {
        return;
    };
    let from_kind = AgentKind::parse_str(original).unwrap_or(AgentKind::Custom);
    let from_custom = AgentKind::parse_str(original)
        .is_none()
        .then_some(original.as_str());
    let wall = wall_label(crate::route_availability::availability(&from_kind, from_custom).wall);
    let _ = store.insert_event(&crate::types::TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: crate::types::EventKind::Milestone,
        detail: held_substitution_detail(original, hold, &setup.agent_display_name, dry_run),
        metadata: Some(held_substitution_metadata(
            original,
            &setup.agent_display_name,
            None,
            setup.effective_model.as_deref(),
            hold,
            wall,
            dry_run,
        )),
    });
}

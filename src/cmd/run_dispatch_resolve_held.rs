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
    *effective_model = None;
    *substituted_from = Some((original, hold));
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
        if rate_limit::dispatch_blocking_hold(kind, candidate_custom).is_none() {
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
        .filter(|fb| rate_limit::dispatch_blocking_hold(fb, None).is_none())
    {
        return Ok((fb, fb.as_str().to_string(), vec![]));
    }
    anyhow::bail!(
        "{held_name} is held ({hold}). Use --cascade <agent> or `aid config clear-limit {held_name}` to clear."
    )
}

// Insert a milestone event when a held route was substituted. No-op if `substituted_from` is None.
pub(crate) fn maybe_insert_held_route_event(
    store: &Store,
    task_id: &crate::types::TaskId,
    setup: &AgentSetup,
) {
    let Some((ref original, ref hold)) = setup.substituted_from else {
        return;
    };
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

// Agent metadata helpers for JSON command and web responses.
// Exports: quota/model helpers and command checks.
// Deps: agent registry types, model catalog, rate limits, and selection scores.

use crate::agent::custom::CustomAgentConfig;
use crate::cmd::agent_json_types::{GroupHoldJson, QuotaJson};
use crate::types::AgentKind;

pub fn command_installed(command: &str) -> bool {
    crate::agent::custom_route_blocker(command).is_none()
}

pub(crate) fn build_quota_json(rlk: &AgentKind, custom_name: Option<&str>) -> QuotaJson {
    let avail = crate::route_availability::availability(rlk, custom_name);
    let quota = crate::agent::selection::quota_from(&avail);
    let window = avail
        .probe
        .as_ref()
        .and_then(|probe| crate::agent::selection::tightest_window(&probe.windows))
        .map(|item| item.label.clone())
        .filter(|label| !label.is_empty());
    let held = avail.status == crate::route_availability::RouteStatus::Held;
    let groups = if held {
        Vec::new()
    } else {
        crate::rate_limit::active_group_holds(rlk, custom_name)
            .into_iter()
            .map(|(group, info)| GroupHoldJson {
                group,
                recovery_at: info.recovery_at,
                message: info.message,
            })
            .collect()
    };
    let state = if held {
        "limited"
    } else if !groups.is_empty() {
        "partial"
    } else if avail.status == crate::route_availability::RouteStatus::Degraded {
        "degraded"
    } else if crate::agent::selection::observed_ok(&avail) {
        "ok"
    } else {
        "unknown"
    };
    let info = held
        .then(|| crate::rate_limit::get_rate_limit_info(rlk, custom_name))
        .flatten();
    QuotaJson {
        state: state.to_string(),
        recovery_at: info.as_ref().and_then(|value| value.recovery_at.clone()),
        message: info.as_ref().and_then(|value| value.message.clone()),
        source: quota.source,
        groups,
        used_percent: quota.used_percent,
        resets_at: quota.resets_at,
        window,
        stale: quota.stale,
    }
}

pub(crate) fn builtin_profile(name: &str) -> Option<AgentKind> {
    crate::agent::routable_builtins().find(|kind| kind.as_str().eq_ignore_ascii_case(name))
}

pub(crate) fn custom_has_endpoint(config: &CustomAgentConfig) -> bool {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|url| !url.is_empty())
}

pub(crate) fn rate_limit_kind(kind: AgentKind, _custom_config: Option<&CustomAgentConfig>) -> AgentKind {
    kind
}

pub(crate) fn metering_label(shape: crate::types::MeteringShape) -> String {
    use crate::types::MeteringShape as M;
    match shape {
        M::AccountPool => "account_pool",
        M::PerModelFamily => "per_model_family",
        M::SpendBudget => "spend_budget",
        M::Subscription => "subscription",
        M::None => "none",
        M::Unknown => "unknown",
    }
    .to_string()
}

#[cfg(test)]
#[path = "agent_json_helpers_tests.rs"]
mod quota_probe_tests;

pub(super) fn agent_metadata(
    kind: AgentKind, custom_config: Option<&CustomAgentConfig>,
) -> (String, String, String, String) {
    // `trust_tier` keeps the JSON field name for callers; the value is the
    // provider-derived egress label (local | private-network | third-party | unknown).
    let result = if let Some(config) = custom_config {
        (
            config.display_name.clone(),
            crate::agent::egress::resolve_agent_egress(&config.id)
                .label()
                .to_string(),
        )
    } else if let Some((_, desc, _, _, _)) = kind.profile() {
        (
            desc.to_string(),
            crate::types::egress_for_cli(kind).label().to_string(),
        )
    } else {
        ("".to_string(), crate::types::EgressTier::Unknown.label().to_string())
    };

    let (provider, metering) = if let Some(config) = custom_config {
        crate::types::provider_for_custom(
            config.provider.as_deref(),
            config.metering.as_deref(),
        )
    } else {
        crate::types::provider_for_cli(kind)
    };

    (result.0, result.1, provider.as_str().to_string(), metering_label(metering))
}

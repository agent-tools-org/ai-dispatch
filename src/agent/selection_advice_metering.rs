// Cross-candidate marginal-spend preference and explicit-run notice.
// Uses existing metering identities and resolved candidate availability only.
use super::{AdviceReport, RankedCandidate};
use crate::types::{AgentKind, MeteringShape, provider_for_cli};

const METERED_PENALTY: f64 = -20.0;

fn shape(name: &str) -> MeteringShape {
    if crate::config::api_route::find(name)
        .ok()
        .flatten()
        .is_some()
    {
        return MeteringShape::SpendBudget;
    }
    if let Some(kind) = AgentKind::parse_str(name) {
        return provider_for_cli(kind).1;
    }
    crate::agent::registry::load_custom_agents()
        .get(name)
        .map(|config| {
            crate::types::provider_for_custom(
                config.provider.as_deref(),
                config.metering.as_deref(),
            )
            .1
        })
        .unwrap_or(MeteringShape::Unknown)
}

fn metered(name: &str) -> bool {
    matches!(
        shape(name),
        MeteringShape::SpendBudget | MeteringShape::None
    )
}

fn prepaid(candidate: &super::AdviceCandidate) -> bool {
    let native_claude = candidate.agent == "claude" && candidate.quota.status == "unknown";
    let plan = matches!(
        shape(&candidate.agent),
        MeteringShape::Subscription | MeteringShape::AccountPool | MeteringShape::PerModelFamily
    );
    let provider_override = candidate.agent == "codex"
        && crate::agent::codex::provider::route_name().map_or(true, |route| route.is_some());
    candidate.installed
        && candidate.eligible
        && !provider_override
        && matches!(candidate.quota.status.as_str(), "dispatchable" | "unknown")
        && (plan || native_claude)
}

pub(super) fn apply(ranked: &mut [RankedCandidate]) {
    let names: Vec<_> = ranked
        .iter()
        .filter(|item| prepaid(&item.report))
        .map(|item| item.report.agent.clone())
        .collect();
    if names.is_empty() {
        return;
    }
    for item in ranked.iter_mut().filter(|item| metered(&item.report.agent)) {
        item.report.breakdown.metering = METERED_PENALTY;
        item.report.breakdown.total += METERED_PENALTY;
        item.report.score += METERED_PENALTY;
        let efficiency_factor = if item.order.score == 0.0 {
            1.0
        } else {
            item.order.efficiency / item.order.score
        };
        item.order.efficiency += METERED_PENALTY * efficiency_factor;
        item.order.score += METERED_PENALTY;
        item.report.metering_reason = Some(format!("usable prepaid routes: {}", names.join(", ")));
    }
}

impl AdviceReport {
    pub(crate) fn metered_notice(&self, name: &str) -> Option<String> {
        if !metered(name) {
            return None;
        }
        let names: Vec<_> = self
            .candidates
            .iter()
            .filter(|candidate| prepaid(candidate))
            .map(|candidate| {
                let used = candidate
                    .quota
                    .used_percent
                    .map(|used| format!("{used:.1}% used"))
                    .unwrap_or_else(|| "usage unknown".into());
                format!("{} ({used})", candidate.agent)
            })
            .collect();
        (!names.is_empty()).then(|| {
            format!(
                "metered route {name} selected; usable prepaid routes: {}",
                names.join(", ")
            )
        })
    }
}

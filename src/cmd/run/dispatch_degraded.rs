// Degradation warnings scoped to the resolved dispatch model.
// Uses route availability and relevant quota windows after family resolution.
use crate::{agent, types::AgentKind};

pub(super) fn warn_if_degraded(kind: AgentKind, custom_name: Option<&str>, model: Option<&str>) {
    if let Some(line) = degraded_dispatch_warning(kind, custom_name, model) {
        aid_warn!("{line}");
    }
}

pub(super) fn degraded_dispatch_warning(
    kind: AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> Option<String> {
    use crate::routing::availability as quota;
    let group = agent::family_route::per_family(kind)
        .then(|| agent::model_group::model_group(kind, model))
        .flatten();
    let mut avail = match group {
        Some(group) => quota::availability_for_group(&kind, custom_name, group),
        None => quota::availability(&kind, custom_name),
    };
    if let Some(group) = group
        && let Some(probe) = avail.probe.as_mut()
    {
        probe.windows = quota::relevant_windows(probe, &kind, Some(group));
        if !probe
            .windows
            .iter()
            .any(|window| window.used_percent >= 80.0)
        {
            return None;
        }
    }
    if avail.status != crate::routing::availability::RouteStatus::Degraded {
        return None;
    }
    let name = custom_name.unwrap_or_else(|| kind.as_str());
    let window = avail
        .probe
        .as_ref()
        .and_then(|probe| crate::agent::selection::tightest_window(&probe.windows));
    Some(format!(
        "[aid] {name} route degraded{} — dispatching anyway",
        window_detail(window)
    ))
}

fn window_detail(window: Option<&crate::routing::availability::WindowView>) -> String {
    match window {
        Some(item) => {
            let label = if item.label.is_empty() {
                "live"
            } else {
                item.label.as_str()
            };
            let resets = item
                .resets_at
                .map(|at| format!(" (resets {})", at.format("%Y-%m-%dT%H:%MZ")))
                .unwrap_or_default();
            format!(": {:.0}% of {label} window used{resets}", item.used_percent)
        }
        None => String::new(),
    }
}

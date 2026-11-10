// Model-scoped quota display and prepaid status for advice candidates.
// Reuses the existing family window selector and availability policy.
use super::*;

pub(in crate::agent::selection) fn candidate_quota(
    kind: AgentKind,
    custom_name: Option<&str>,
    model: Option<&str>,
) -> CandidateQuota {
    // A family-pinned model reads only its family marker; an agent-wide hold still covers it.
    let agent_wide = crate::routing::availability::availability(&kind, custom_name);
    if agent_wide.status == RouteStatus::Held {
        return quota_from(&agent_wide);
    }
    let mut avail = availability_for_model(&kind, custom_name, model);
    if let Some(group) = crate::agent::model_group::model_group(kind, model)
        && let Some(probe) = avail.probe.as_mut()
    {
        probe.windows = relevant_windows(probe, &kind, Some(group));
        if avail.marker.is_none() {
            avail = crate::routing::availability::decide(
                &kind,
                custom_name,
                Some(group),
                None,
                None,
                avail.probe.clone(),
            );
        }
    }
    quota_from(&avail)
}

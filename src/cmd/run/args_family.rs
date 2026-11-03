// Shared family routing policy for dispatch and saved worker arguments.
// Reads explicit continuation provenance; parent lookup is only for self-heal.
use super::*;

impl RunArgs {
    pub(crate) fn family_route_inputs(
        &self,
        store: &Store,
        kind: crate::types::AgentKind,
        custom: Option<&str>,
    ) -> Result<(crate::agent::family_route::PinPolicy, Option<String>)> {
        use crate::agent::family_route::PinPolicy;
        let policy = if self.quota_continuation_of.is_some() {
            PinPolicy::Continuation
        } else if self.policy.is_some()
            || (self.model.is_some() && self.model_source == ModelSource::Advised)
        {
            PinPolicy::Exact
        } else if self.model.is_some() && self.model_source == ModelSource::UserSupplied {
            PinPolicy::WarnOnReplacement
        } else {
            PinPolicy::Automatic
        };
        let excluded = if let Some(model) = &self.quota_continuation_of {
            Some(model.clone())
        } else if self.force_default_model {
            self.parent_task_id
                .as_deref()
                .map(|id| store.get_task(id))
                .transpose()?
                .flatten()
                .filter(|task| task.agent == kind && task.custom_agent_name.as_deref() == custom)
                .and_then(|task| task.requested_model.or(task.observed_model))
        } else {
            None
        };
        Ok((policy, excluded))
    }
}

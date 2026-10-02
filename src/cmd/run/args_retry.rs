// Retry arguments use saved dispatch inputs before legacy row declarations.
// Implements RunArgs::for_retry; deps: Store, Task and model provenance.
use super::*;

impl RunArgs {
    pub(crate) fn for_retry(store: &Store, task: &crate::types::Task) -> Result<Self> {
        let saved = Self::saved_for_task(store, task.id.as_str())?;
        let profile = if saved.is_none() {
            store.get_task_profile(task.id.as_str())?
        } else {
            Default::default()
        };
        let mut args = saved.unwrap_or_else(|| Self {
            kind: task
                .category
                .as_deref()
                .and_then(crate::agent::classifier::TaskCategory::parse_str),
            declared_difficulty: profile.difficulty,
            declared_budget: profile.budget,
            declared_urgency: profile.urgency,
            declared_rigor: profile.rigor,
            repo: task.repo_path.clone(),
            dir: task.repo_path.clone(),
            output: task.output_path.clone(),
            model: task.requested_model.clone(),
            model_source: ModelSource::AidResolved,
            group: task.workgroup_id.clone(),
            verify: task.verify.clone(),
            read_only: task.read_only,
            budget: task.budget,
            ..Default::default()
        });
        args.agent_name = task.agent_display_name().to_string();
        args.repo = args.repo.or_else(|| task.repo_path.clone());
        if args.model_source != ModelSource::UserSupplied {
            args.model = task.requested_model.clone();
        }
        args.session_id = if task.agent.supports_session_resume() {
            task.agent_session_id.clone()
        } else {
            None
        };
        args.existing_task_id = None;
        args.env = None;
        Ok(args)
    }
}

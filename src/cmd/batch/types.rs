// Batch types shared across batch submodules.
// Exports: BatchTaskOutcome, DispatchedTask, CompletedTask, BatchDispatchResult
// Deps: std
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BatchTaskOutcome {
    Done,
    Failed,
    Skipped,
}

pub(crate) struct DispatchedTask {
    pub(crate) index: usize,
    pub(crate) task_id: Option<String>,
}

pub(crate) struct CompletedTask {
    pub(crate) index: usize,
    pub(crate) task_id: String,
    pub(crate) outcome: BatchTaskOutcome,
}

pub(crate) struct BatchDispatchResult {
    pub(crate) task_ids: Vec<String>,
    pub(crate) outcomes: Vec<BatchTaskOutcome>,
}

impl BatchDispatchResult {
    pub(crate) fn dispatched_task_ids(&self) -> Vec<String> {
        self.task_ids
            .iter()
            .zip(&self.outcomes)
            .filter_map(|(task_id, outcome)| match outcome {
                BatchTaskOutcome::Done | BatchTaskOutcome::Failed => Some(task_id.clone()),
                BatchTaskOutcome::Skipped => None,
            })
            .collect()
    }
}

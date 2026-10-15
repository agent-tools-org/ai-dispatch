// Command-neutral task action services for UI/API callers.
// Exports stop, retry, and merge entry points; CLI modules remain output adapters.
// Deps: cmd action implementations, Store, TaskId.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd;
use crate::store::Store;
use crate::types::TaskId;

pub use cmd::retry::RetryArgs;

pub struct MergeArgs<'a> {
    pub task_id: Option<&'a str>,
    pub group: Option<&'a str>,
    pub approve: bool,
    pub check: bool,
    pub force: bool,
    pub target: Option<&'a str>,
    pub lanes: bool,
}

pub fn stop(store: &Arc<Store>, task_id: &str) -> Result<()> {
    cmd::stop::terminate_any(store, task_id)
}

pub async fn retry(store: Arc<Store>, args: RetryArgs) -> Result<TaskId> {
    cmd::retry::retry_task(store, args, false).await
}

pub fn merge(store: Arc<Store>, args: MergeArgs<'_>) -> Result<()> {
    cmd::merge::run_quiet(
        store,
        args.task_id,
        args.group,
        args.approve,
        args.check,
        args.force,
        args.target,
        args.lanes,
    )
}

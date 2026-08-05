// Owner-chain re-entry checks for nested task worktree leases.
// Exports: holder_allows_reentry().
// Deps: Store parent_task_id ancestry walk.

use crate::store::Store;

/// True when `requester_id` may share a lease held by `holder_id`.
/// Same-task refresh is allowed; otherwise `holder_id` must be an ancestor.
pub(super) fn holder_allows_reentry(
    store: Option<&Store>,
    requester_id: &str,
    holder_id: &str,
) -> bool {
    if requester_id == holder_id {
        return true;
    }
    let Some(store) = store else {
        return false;
    };
    let mut current = match store.get_task(requester_id) {
        Ok(Some(task)) => task.parent_task_id,
        _ => return false,
    };
    for _ in 0..32 {
        let Some(parent_id) = current else {
            return false;
        };
        if parent_id == holder_id {
            return true;
        }
        current = match store.get_task(&parent_id) {
            Ok(Some(task)) => task.parent_task_id,
            _ => None,
        };
    }
    false
}

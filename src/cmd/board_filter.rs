// Project-scope filtering for `aid board` and stream views.
// Exports: apply_board_filters, active_project_filter.
// Deps: project identity helpers, session, store Task list.

use crate::session;
use crate::types::Task;

/// Apply mine/group/project filters in memory (no per-row DB queries).
/// When `all_projects` is false, keeps only the current project identity
/// (or the unattributed bucket when none resolves).
pub(crate) fn apply_board_filters(
    tasks: &mut Vec<Task>,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
) -> Option<Option<String>> {
    if mine {
        tasks.retain(session::matches_current);
    }
    if let Some(group_id) = group {
        tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    }
    if all_projects {
        return None;
    }
    let current = crate::project::current_project_id();
    crate::project::retain_project(tasks, current.as_deref());
    Some(current)
}

/// Banner line for the active project filter (always shown on human board).
pub(crate) fn project_scope_banner(
    project_filter: Option<Option<&str>>,
    all_projects: bool,
) -> String {
    let filter = project_filter.unwrap_or(None);
    format!(
        "[aid] {}",
        crate::project::project_filter_banner(filter, all_projects)
    )
}

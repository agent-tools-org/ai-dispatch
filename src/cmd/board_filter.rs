// Project-scope filtering for `aid board` and stream views.
// Exports: load_board_tasks and project scope display.
// Deps: project identity helpers, session, store Task list.

use crate::runtime::session;
use crate::types::Task;

pub(crate) type BoardSelection = (Vec<Task>, Option<Option<String>>, Option<super::TruncationNotice>);

pub(crate) fn load_board_tasks(
    store: &crate::store::Store,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    all_projects: bool,
    limit: Option<usize>,
) -> anyhow::Result<BoardSelection> {
    let project = (!all_projects).then(crate::config::project::current_project_id);
    let caller = session::current_caller();
    let scope = crate::store::TaskReadFilter {
        group,
        project: project.as_ref().map(|value| value.as_deref()),
        caller: mine.then_some(caller.as_ref()),
    };
    let limit = effective_limit(limit, running, today, mine, group);
    let (tasks, total) = store.read_board_tasks(super::board_task_filter(running, today), &scope, limit)?;
    let truncation = limit
        .filter(|limit| total > *limit)
        .map(|shown| super::TruncationNotice { shown, total });
    Ok((tasks, project, truncation))
}

pub(super) fn effective_limit(
    limit: Option<usize>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
) -> Option<usize> {
    limit.or_else(|| (group.is_none() && !running && !today && !mine).then_some(super::DEFAULT_TASK_LIMIT))
}

/// Banner line for the active project filter (always shown on human board).
pub(crate) fn project_scope_banner(project_filter: Option<Option<&str>>, all_projects: bool) -> String {
    let filter = project_filter.unwrap_or(None);
    format!(
        "[aid] {}",
        crate::config::project::project_filter_banner(filter, all_projects)
    )
}

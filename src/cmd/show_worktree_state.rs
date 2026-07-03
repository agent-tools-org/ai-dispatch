// Live worktree-state rendering for `aid show`.
// Exports the shared section used by summary and diff views.
// Deps: worktree live-state capture.

pub(crate) fn worktree_state_section(
    worktree_path: &str,
    state: Option<&crate::worktree::LiveWorktreeState>,
) -> String {
    let Some(state) = state else { return String::new() };
    let mut out = String::new();
    out.push_str("\n--- Worktree State ---\n");
    out.push_str(&state.summary_text());
    out.push('\n');
    if state.is_dirty() {
        out.push_str(&format!(
            "Partial work present (uncommitted) at {worktree_path}\n"
        ));
        out.push_str(&state.dirty_stat_text());
    }
    out
}

// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn worktree_prefix_generates_worktree_for_unnamed_tasks() {
    let (cfg, _) = parse_batch_with_vars(
        concat!(
            "[defaults]\nworktree_prefix = \"feat\"\nagent = \"codex\"\n",
            "[[task]]\nprompt = \"unnamed task\"\n"
        ),
        &[],
    );
    assert_eq!(
        cfg.tasks[0].worktree.as_deref(),
        Some("feat/task-0"),
        "unnamed task should get index-based worktree"
    );
}

#[test]
fn worktree_prefix_prefers_name_over_index() {
    let (cfg, _) = parse_batch_with_vars(
        concat!(
            "[defaults]\nworktree_prefix = \"feat\"\nagent = \"codex\"\n",
            "[[task]]\nname = \"impl\"\nprompt = \"named task\"\n"
        ),
        &[],
    );
    assert_eq!(
        cfg.tasks[0].worktree.as_deref(),
        Some("feat/impl"),
        "named task should use name, not index"
    );
}

// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn worktree_prefix_uses_task_name_or_index() {
    for (config, expected, value_3) in [
        (
            "[[task]]\nprompt = \"unnamed task\"\n",
            "feat/task-0",
            "unnamed task should get index-based worktree",
        ),
        (
            "[[task]]\nname = \"impl\"\nprompt = \"named task\"\n",
            "feat/impl",
            "named task should use name, not index",
        ),
    ] {
        eprintln!("case: {:?}", (config, expected, value_3));
        let (cfg, _) = parse_batch_with_vars(
            concat!(
                "[defaults]\nworktree_prefix = \"feat\"\nagent = \"codex\"\n",
                config
            ),
            &[],
        );
        assert_eq!(
            cfg.tasks[0].worktree.as_deref(),
            Some(expected),
            value_3
        );
    }
}



// Batch dispatch concurrency: effective_max_active() for the dispatch loop.
// Exports: effective_max_active()
// Deps: crate::batch, crate::store::Store

use crate::batch;
use crate::store::Store;

pub(super) fn effective_max_active(
    _store: &Store,
    tasks: &[batch::BatchTask],
    _ready: &[usize],
    _active: &[(usize, String)],
    max_concurrent: Option<usize>,
) -> anyhow::Result<usize> {
    if let Some(limit) = max_concurrent {
        return Ok(limit.max(1));
    }
    // Default: use system resource recommendation capped at task count.
    // Previously this was further capped by unique_effective_agent_count,
    // which serialized all-same-agent batches to 1 (#100).
    Ok(crate::system_resources::recommended_max_concurrent()
        .min(tasks.len())
        .max(1))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn make_task(agent: &str, fallback: Option<&str>) -> batch::BatchTask {
        batch::BatchTask {
            id: None,
            name: None,
            agent: agent.to_string(),
            team: None,
            prompt: "prompt".to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None,
            verify: None,
            setup: None,
            judge: None,
            peer_review: None,
            best_of: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: fallback.map(str::to_string),
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            budget: false,
            audit: None,
            env: None,
            env_forward: None,
            worktree_link_deps: None,
            on_success: None,
            on_fail: None,
            conditional: false,
        }
    }

    #[test]
    fn explicit_max_concurrent_is_always_respected() {
        let store = Store::open_memory().unwrap();
        let tasks = [make_task("codex", Some("cursor"))];

        let limit = effective_max_active(&store, &tasks, &[0], &[], Some(7)).unwrap();

        assert_eq!(limit, 7);
    }

    #[test]
    fn auto_limit_does_not_serialize_same_agent_tasks() {
        let store = Store::open_memory().unwrap();
        let tasks = [
            make_task("codex", None),
            make_task("codex", None),
            make_task("codex", None),
        ];

        let limit = effective_max_active(&store, &tasks, &[0, 1, 2], &[], None).unwrap();

        // Should allow all 3 tasks to run, not serialize to 1
        assert!(limit >= 3, "expected >= 3 but got {limit}");
    }

    #[test]
    fn auto_limit_caps_at_task_count() {
        let store = Store::open_memory().unwrap();
        let tasks = [make_task("codex", None), make_task("codex", None)];

        let limit = effective_max_active(&store, &tasks, &[0, 1], &[], None).unwrap();

        assert_eq!(limit, 2);
    }
}

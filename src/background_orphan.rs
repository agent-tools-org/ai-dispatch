// Orphaned background-task staleness cleanup.
// Exports cleanup_orphaned_idle_tasks for background zombie reconciliation.
// Deps: background specs, idle timeout defaults, store events, and task types.

use anyhow::Result;
use chrono::{DateTime, Local};

use super::background_process::kill_process;
use super::background_spec::load_spec_if_exists;
use crate::cmd::run_hung_recovery;
use crate::idle_timeout::DEFAULT_IDLE_TIMEOUT_SECS;
use crate::store::Store;
use crate::types::{Task, TaskEvent, TaskId};

pub(super) fn cleanup_orphaned_idle_tasks<F>(
    store: &Store,
    running_tasks: &[Task],
    already_cleaned: &[String],
    is_process_alive: &F,
) -> Result<Vec<String>>
where
    F: Fn(u32) -> bool,
{
    let now = Local::now();
    let mut cleaned = Vec::new();
    for task in running_tasks {
        let task_id = task.id.as_str();
        if already_cleaned.iter().any(|id| id == task_id) {
            continue;
        }
        let Some(spec) = load_spec_if_exists(task_id)? else {
            continue;
        };
        if spec.worker_pid.is_some_and(is_process_alive) {
            continue;
        }
        let idle_secs = spec.idle_timeout_secs.unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);
        let activity = latest_activity(store, task)?;
        if !is_stale(activity.timestamp, now, idle_secs) {
            continue;
        }
        if let Some(agent_pid) = spec.agent_pid {
            kill_process(agent_pid);
        }
        if record_orphaned_hung(store, task_id, idle_secs, &activity)? {
            cleaned.push(task_id.to_string());
        }
    }
    Ok(cleaned)
}

pub(super) fn latest_activity(store: &Store, task: &Task) -> Result<TaskActivity> {
    let events = store.get_events(task.id.as_str())?;
    let progress_events = events.iter()
        .filter(|event| event.event_kind.is_progress() && !is_idle_bookkeeping_event(event))
        .collect::<Vec<_>>();
    let last_event = progress_events.last();
    Ok(TaskActivity {
        timestamp: last_event.map(|event| event.timestamp).unwrap_or(task.created_at),
        event_count: progress_events.len() as u32,
        detail: last_event.map(|event| event.detail.clone()),
    })
}

pub(super) fn is_stale(last_activity: DateTime<Local>, now: DateTime<Local>, idle_secs: u64) -> bool {
    (now - last_activity).num_seconds() >= idle_secs as i64
}

fn is_idle_bookkeeping_event(event: &TaskEvent) -> bool {
    let Some(metadata) = event.metadata.as_ref() else {
        return false;
    };
    metadata.get("idle_warn").and_then(|value| value.as_bool()) == Some(true)
        || metadata.get("auto_escalated").and_then(|value| value.as_bool()) == Some(true)
        || metadata.get("source").and_then(|value| value.as_str()) == Some("unstick-auto")
}

fn record_orphaned_hung(
    store: &Store,
    task_id: &str,
    idle_secs: u64,
    activity: &TaskActivity,
) -> Result<bool> {
    let detail = format!("hung detected (orphaned supervisor): no output for {idle_secs}s");
    record_hung_detected_failure(store, task_id, idle_secs, activity, &detail)
}

pub(super) fn record_hung_detected_failure(
    store: &Store,
    task_id: &str,
    idle_secs: u64,
    activity: &TaskActivity,
    detail: &str,
) -> Result<bool> {
    if !super::record_failure(store, task_id, &detail, &detail)? {
        return Ok(false);
    }
    run_hung_recovery::insert_hung_detected_events(
        store,
        &TaskId(task_id.to_string()),
        idle_secs,
        activity.event_count,
        activity.detail.as_deref(),
    )?;
    Ok(true)
}

pub(super) struct TaskActivity {
    pub(super) timestamp: DateTime<Local>,
    pub(super) event_count: u32,
    pub(super) detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::{save_spec, BackgroundRunSpec};
    use crate::paths;
    use crate::store::Store;
    use crate::types::{
        AgentKind, EventKind, Task, TaskEvent, TaskStatus, VerifyStatus,
    };

    fn make_task(task_id: &str) -> Task {
        Task {
            id: TaskId(task_id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Running,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    fn make_spec(task_id: &str, worker_pid: Option<u32>, idle_timeout_secs: Option<u64>) -> BackgroundRunSpec {
        BackgroundRunSpec {
            task_id: task_id.to_string(),
            worker_pid,
            agent_name: "codex".to_string(),
            prompt: "prompt".to_string(),
            dir: Some(".".to_string()),
            output: None,
            result_file: None,
            model: None,
            verify: None,
            setup: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            judge: None,
            max_duration_mins: None,
            idle_timeout_secs,
            retry: 0,
            group: None,
            skills: vec![],
            checklist: vec![],
            template: None,
            interactive: true,
            on_done: None,
            cascade: vec![],
            parent_task_id: None,
            env: None,
            env_forward: None,
            agent_pid: None,
            sandbox: false,
            read_only: false,
            audit_report_mode: false,
            container: None,
            link_deps: true,
            pre_task_dirty_paths: None,
        }
    }

    fn insert_event(store: &Store, task_id: &str, age_secs: i64) {
        store
            .insert_event(&TaskEvent {
                task_id: TaskId(task_id.to_string()),
                timestamp: Local::now() - chrono::Duration::seconds(age_secs),
                event_kind: EventKind::Milestone,
                detail: "last progress".to_string(),
                metadata: None,
            })
            .expect("insert event");
    }

    #[test]
    fn orphan_reaper_fails_stale_task_when_worker_is_dead() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        paths::ensure_dirs().expect("ensure dirs");
        let store = Store::open_memory().expect("store");
        let task = make_task("t-orph1");
        store.insert_task(&task).expect("insert task");
        save_spec(&make_spec("t-orph1", Some(77), Some(120))).expect("save spec");
        insert_event(&store, "t-orph1", 121);

        let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

        assert_eq!(cleaned, vec!["t-orph1".to_string()]);
        assert_eq!(
            store.get_task("t-orph1").expect("get task").expect("task").status,
            TaskStatus::Failed
        );
        let events = store.get_events("t-orph1").expect("events");
        assert!(events.iter().any(|event| event.detail.contains("orphaned supervisor")));
        assert!(events.iter().any(|event| event.detail == "hung_detected"));
    }

    #[test]
    fn orphan_reaper_keeps_stale_task_when_worker_is_alive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        paths::ensure_dirs().expect("ensure dirs");
        let store = Store::open_memory().expect("store");
        let task = make_task("t-live1");
        store.insert_task(&task).expect("insert task");
        save_spec(&make_spec("t-live1", Some(77), Some(120))).expect("save spec");
        insert_event(&store, "t-live1", 1_000);

        let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|pid| pid == 77).expect("cleanup");

        assert!(cleaned.is_empty());
        assert_eq!(
            store.get_task("t-live1").expect("get task").expect("task").status,
            TaskStatus::Running
        );
    }

    #[test]
    fn orphan_reaper_uses_spec_idle_timeout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        paths::ensure_dirs().expect("ensure dirs");
        let store = Store::open_memory().expect("store");
        let task = make_task("t-idle1");
        store.insert_task(&task).expect("insert task");
        save_spec(&make_spec("t-idle1", Some(77), Some(600))).expect("save spec");
        insert_event(&store, "t-idle1", 500);

        let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

        assert!(cleaned.is_empty());
        assert_eq!(
            store.get_task("t-idle1").expect("get task").expect("task").status,
            TaskStatus::Running
        );
    }

    #[test]
    fn orphan_reaper_skips_tasks_without_background_spec() {
        let store = Store::open_memory().expect("store");
        let task = make_task("t-nospec");
        store.insert_task(&task).expect("insert task");
        insert_event(&store, "t-nospec", 1_000);

        let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

        assert!(cleaned.is_empty());
        assert_eq!(
            store.get_task("t-nospec").expect("get task").expect("task").status,
            TaskStatus::Running
        );
    }

    #[test]
    fn is_stale_requires_idle_timeout_to_elapse() {
        let now = Local::now();

        assert!(is_stale(now - chrono::Duration::seconds(300), now, 300));
        assert!(!is_stale(now - chrono::Duration::seconds(299), now, 300));
    }
}

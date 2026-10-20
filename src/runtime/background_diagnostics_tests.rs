// Reaper diagnostic preservation and recorded pending-reason precedence.
// Uses isolated task files and an in-memory store.
use super::*;

#[test]
fn reaper_appends_without_losing_agent_stderr() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let id = "t-preserve-stderr";
    store
        .insert_task(&task(id, crate::types::TaskStatus::Running))
        .expect("insert");
    save_spec(&spec(id)).expect("spec");
    std::fs::write(paths::stderr_path(id), "original agent error\n").expect("stderr");
    check_zombie_tasks_with(&store, |_| false).expect("reap");
    let stderr = std::fs::read_to_string(paths::stderr_path(id)).expect("read");
    assert!(stderr.starts_with("original agent error\nworker disappeared; cause unknown"));
    assert!(stderr.contains("worker.log"));
    assert!(!stderr.contains("timeout"));
}

#[test]
fn recorded_preparation_reason_survives_malformed_spec() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let mut pending = task("t-preparation-stall", crate::types::TaskStatus::Pending);
    pending.created_at = chrono::Local::now() - chrono::Duration::seconds(700);
    pending.pending_reason = Some("preparation".into());
    store.insert_task(&pending).expect("insert");
    std::fs::write(paths::job_path(pending.id.as_str()), "invalid spec").expect("spec");
    let cleaned =
        crate::runtime::background::cleanup_stale_pending_tasks(&store).expect("reap pending");
    assert_eq!(cleaned, vec![pending.id.to_string()]);
    let row = store
        .get_task(pending.id.as_str())
        .expect("get")
        .expect("row");
    assert_eq!(row.pending_reason.as_deref(), Some("preparation"));
    assert!(
        store.get_events(pending.id.as_str()).expect("events")[0]
            .detail
            .contains("reason: preparation")
    );
}

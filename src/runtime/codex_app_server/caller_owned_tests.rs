// Caller-owned app-server failures must end the attempt without exec fallback.
// Exercises the same fake handshake and production runner as ordinary fallback tests.
use super::*;
use super::fixture::Fixture;

#[test]
fn caller_owned_gates_app_server_exec_fallback() {
    let fixture = Fixture::new("invalid");
    let policy = crate::store::execution_protocol::ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: "codex".into(),
        model: "exact".into(),
        accepted_at: "2026-10-29T10:00:00Z".into(),
    };
    fixture
        .store
        .db()
        .execute(
            "UPDATE tasks SET execution_policy = ?1 WHERE id = ?2",
            rusqlite::params![
                serde_json::to_string(&policy).expect("policy"),
                fixture.task.as_str()
            ],
        )
        .expect("persist policy");
    fixture.dispatch();
    let task = fixture
        .store
        .get_task(fixture.task.as_str())
        .expect("query")
        .expect("task");
    assert_eq!(task.status, TaskStatus::Failed);
    let events = fixture.store.get_events(fixture.task.as_str()).expect("events");
    assert!(events.iter().any(|event| event.event_kind == EventKind::Error));
    assert!(!events.iter().any(|event| {
        event
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata["codex_app_server_fallback"] == true)
    }));
}

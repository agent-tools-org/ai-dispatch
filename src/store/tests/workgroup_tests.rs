// Workgroup-focused Store tests.
// Exports: workgroup query/mutation tests.
// Deps: Store.

use super::*;

#[test]
fn create_and_get_workgroup() {
    let store = Store::open_memory().unwrap();
    let workgroup = store
        .create_workgroup("dispatch", "Shared API contract context.", None, None)
        .unwrap();

    let loaded = store.get_workgroup(workgroup.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.id, workgroup.id);
    assert_eq!(loaded.name, "dispatch");
    assert!(loaded.shared_context.contains("API contract"));
}

#[test]
fn gets_workgroup_milestones() {
    let store = Store::open_memory().unwrap();
    let workgroup = store
        .create_workgroup("dispatch", "Shared API contract context.", None, None)
        .unwrap();

    let mut first = make_task("t-0040", AgentKind::Codex, TaskStatus::Running);
    first.workgroup_id = Some(workgroup.id.as_str().to_string());
    store.insert_task(&first).unwrap();

    let mut second = make_task("t-0041", AgentKind::Grok, TaskStatus::Running);
    second.workgroup_id = Some(workgroup.id.as_str().to_string());
    store.insert_task(&second).unwrap();

    let mut other = make_task("t-0042", AgentKind::Cursor, TaskStatus::Running);
    other.workgroup_id = Some("wg-other".to_string());
    store.insert_task(&other).unwrap();

    store
        .insert_event(&TaskEvent::at(
            TaskId("t-0040".to_string()),
            EventKind::Milestone,
            "finding one".to_string(),
            Local::now() - chrono::Duration::seconds(3),
        ))
        .unwrap();
    store
        .insert_event(&TaskEvent::at(
            TaskId("t-0040".to_string()),
            EventKind::ToolCall,
            "ignored".to_string(),
            Local::now() - chrono::Duration::seconds(2),
        ))
        .unwrap();
    store
        .insert_event(&TaskEvent::at(
            TaskId("t-0041".to_string()),
            EventKind::Milestone,
            "finding two".to_string(),
            Local::now() - chrono::Duration::seconds(1),
        ))
        .unwrap();
    store
        .insert_event(&TaskEvent::new(
            TaskId("t-0042".to_string()),
            EventKind::Milestone,
            "other group".to_string(),
        ))
        .unwrap();

    let milestones = store
        .get_workgroup_milestones(workgroup.id.as_str())
        .unwrap();
    assert_eq!(
        milestones,
        vec![
            ("t-0040".to_string(), "finding one".to_string()),
            ("t-0041".to_string(), "finding two".to_string()),
        ]
    );
}

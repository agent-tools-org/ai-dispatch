// JSON show shares ordinary command startup and persisted receipt rendering.
// Other commands retain the default startup store path.
use super::*;
use clap::Parser;

#[test]
fn show_json_initializes_a_missing_store_like_other_commands() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let _command = Cli::try_parse_from(["aid", "show", "t-missing", "--json"])
        .expect("parse")
        .command;
    assert!(command_store().is_ok());
    assert!(support::paths::db_path().exists());
}

#[test]
fn show_json_opens_a_writable_store_and_runs_normal_startup() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let store = store::Store::open(&support::paths::db_path()).expect("store");
    store
        .insert_task(&types::Task::test_fixture(
            types::TaskId("t-show-startup".into()),
            types::TaskStatus::Running,
        ))
        .expect("task");
    runtime::background::save_spec(&serde_json::from_value(serde_json::json!({
        "task_id":"t-show-startup", "worker_pid":u32::MAX
    })).expect("dead worker spec")).expect("spec");
    drop(store);
    let _command = Cli::try_parse_from(["aid", "show", "t-show-startup", "--json"])
        .expect("parse")
        .command;
    let opened = command_store().expect("normal startup store");
    assert!(
        opened
            .db()
            .execute("UPDATE tasks SET prompt='observed' WHERE id='t-show-startup'", [])
            .is_ok()
    );
    assert_eq!(
        opened
            .get_task("t-show-startup")
            .expect("row")
            .expect("task")
            .status,
        types::TaskStatus::Failed
    );
}

// JSON show opens no writable store or reaper path, even before command dispatch.
// Other commands retain the default startup store path.
use super::*;

#[test]
fn caller_owned_show_json_does_not_create_a_missing_store() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let command = Cli::try_parse_from(["aid", "show", "t-missing", "--json"])
        .expect("parse")
        .command;
    assert!(command_store(&command).is_err());
    assert!(!support::paths::db_path().exists());
}

#[test]
fn caller_owned_show_json_opens_an_existing_store_read_only_without_reaping() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let store = store::Store::open(&support::paths::db_path()).expect("store");
    store
        .insert_task(&types::Task::test_fixture(
            types::TaskId("t-show-startup".into()),
            types::TaskStatus::Running,
        ))
        .expect("task");
    drop(store);
    let command = Cli::try_parse_from(["aid", "show", "t-show-startup", "--json"])
        .expect("parse")
        .command;
    let read_only = command_store(&command).expect("read only store");
    assert!(
        read_only
            .db()
            .execute("UPDATE tasks SET status='failed' WHERE id='t-show-startup'", [])
            .is_err()
    );
    assert_eq!(
        read_only
            .get_task("t-show-startup")
            .expect("row")
            .expect("task")
            .status,
        types::TaskStatus::Running
    );
}

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

#[test]
fn default_json_admission_refuses_before_startup_reaping_or_store_writes() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let store = store::Store::open(&support::paths::db_path()).expect("store");
    let id = "t-default-admission";
    store.insert_task(&types::Task::test_fixture(types::TaskId(id.into()), types::TaskStatus::Running))
        .expect("task");
    runtime::background::save_spec(&serde_json::from_value(serde_json::json!({
        "task_id":id, "worker_pid":u32::MAX
    })).expect("dead worker")).expect("spec");
    let before = store.execution_snapshot(id).expect("snapshot");
    for command in ["wait", "stop"] {
        let cli = Cli::try_parse_from(["aid", command, id, "--json"]).expect("parse");
        let error = check_execution_policy(&cli.command).expect_err("default policy refusal");
        assert_eq!(error.downcast_ref::<execution::Refusal>().expect("refusal").exit, 3);
    }
    assert_eq!(store.execution_snapshot(id).expect("snapshot"), before);
    assert_eq!(store.get_task(id).expect("task").expect("row").status, types::TaskStatus::Running);
}

#[test]
fn legacy_json_admission_returns_a_protocol_body_without_migrating() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let connection = rusqlite::Connection::open(support::paths::db_path()).expect("legacy store");
    connection.execute_batch("CREATE TABLE tasks (id TEXT PRIMARY KEY); PRAGMA user_version=3;")
        .expect("schema 3");
    for command in ["wait", "stop"] {
        let cli = Cli::try_parse_from(["aid", command, "t-legacy", "--json"]).expect("parse");
        let error = check_execution_policy(&cli.command).expect_err("protocol refusal");
        let refusal = error.downcast_ref::<execution::Refusal>().expect("structured error");
        assert_eq!(refusal.exit, 3);
        assert_eq!(refusal.body()["error"]["code"], "policy_unsupported");
        assert_eq!(refusal.body()["error"]["detail"], "schema_upgrade_required");
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 3);
}

#[test]
fn run_json_requires_an_explicit_policy() {
    let Err(error) = Cli::try_parse_from(["aid", "run", "codex", "prompt", "--json"]) else {
        panic!("policy required");
    };
    assert_eq!(error.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    assert!(Cli::try_parse_from(["aid", "run", "codex", "prompt"]).is_ok());
    assert!(Cli::try_parse_from(["aid", "run", "codex", "prompt", "--json",
        "--policy", "caller-owned", "--id", "t-cli", "--model", "gpt-5.5"]).is_ok());
}

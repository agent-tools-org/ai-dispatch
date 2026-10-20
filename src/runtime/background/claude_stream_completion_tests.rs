// Piped completion must refresh project state and retain the PTY summary event.
// Runs the production fixture in a child test process to isolate its current directory.
use super::*;

#[tokio::test]
async fn piped_completion_refreshes_project_state_and_records_summary() {
    if launch_isolated_child() {
        return;
    }
    let (_home, _guard, store, spec, args) = setup();
    let cwd = std::env::current_dir().unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET repo_path=?1 WHERE id=?2",
            rusqlite::params![cwd.display().to_string(), spec.task_id],
        )
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        run_process(
            &store,
            &spec,
            &args,
            &ClaudeAgent,
            command("summary", &cwd.join("input")),
            std::time::Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let state: crate::store::state::ProjectState =
        toml::from_str(&std::fs::read_to_string(cwd.join(".aid/state.toml")).unwrap()).unwrap();
    assert_eq!(state.health.total_tasks, 1);
    assert_eq!(
        state.context.last_task_id.as_deref(),
        Some(spec.task_id.as_str())
    );
    assert_eq!(state.performance.avg_task_cost_usd, Some(0.25));
    assert_summary(&store, &spec.task_id);
}

fn launch_isolated_child() -> bool {
    if std::env::var_os("AID_TEST_PIPED_COMPLETION_CHILD").is_some() {
        return false;
    }
    let _permit = crate::support::test_subprocess::acquire();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::create_dir(cwd.path().join(".aid")).unwrap();
    let name = format!(
        "{}::piped_completion_refreshes_project_state_and_records_summary",
        module_path!()
    );
    let (_, test) = name.split_once("::").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("AID_TEST_PIPED_COMPLETION_CHILD", "1")
        .current_dir(cwd.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    true
}

fn assert_summary(store: &Store, task_id: &str) {
    let events = store.get_events(task_id).unwrap();
    let summaries: Vec<_> = events
        .iter()
        .filter(|event| {
            event.event_kind == crate::types::EventKind::Completion
                && event.detail.starts_with("DONE (")
        })
        .collect();
    assert_eq!(summaries.len(), 1);
    let task = store.get_task(task_id).unwrap().unwrap();
    let seconds = task.duration_ms.unwrap() / 1000;
    assert_eq!(
        summaries[0].detail,
        format!("DONE ({seconds}s, 7 tokens, $0.25)")
    );
}

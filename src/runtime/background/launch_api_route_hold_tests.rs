// Launch-time credential holds across stop, reaper, and verified completion.
// Uses the real launch fixture and existing operator clear-limit handler.
use super::*;

fn next_launch(home: &std::path::Path, store: &Arc<Store>) -> anyhow::Result<()> {
    let mut next = spec(false);
    next.task_id = "t-api-next".into();
    if store.get_task(&next.task_id)?.is_none() {
        store.insert_waiting_task(
            &next.task_id,
            "claude-api-ming",
            "test",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )?;
        crate::task::lifecycle::mark_running(store, &crate::types::TaskId(next.task_id.clone()))?;
    }
    let args = args(home);
    let agent = agent::registry::resolve_custom_agent(&args.agent_name)
        .ok_or_else(|| anyhow::anyhow!("route agent did not resolve"))?;
    let isolated = agent::home_isolation::IsolatedHomeGuard::create_from_home(
        Some(home),
        Some(&next.task_id),
    )?;
    prepare_launch(store, &next, &args, agent.as_ref(), &isolated)?;
    Ok(())
}

fn sweep_count(store: &Store) -> usize {
    store
        .get_events("t-api-launch")
        .unwrap()
        .iter()
        .filter(|event| {
            event.event_kind == crate::types::EventKind::Error
                && event
                    .metadata
                    .as_ref()
                    .is_some_and(|value| value["api_route_unverified_held"] == true)
        })
        .count()
}

pub(super) fn assert_next_launch_held_then_cleared(home: &std::path::Path, store: &Arc<Store>) {
    let error = next_launch(home, store).unwrap_err().to_string();
    assert!(error.contains("route claude-api-ming is held"), "{error}");
    assert!(error.contains("not billed to its API key"), "{error}");
    assert_eq!(sweep_count(store), 1);
    let hold =
        std::fs::read_to_string(paths::aid_dir().join("rate-limit-claude-api-ming")).unwrap();
    assert!(hold.contains("hold: manual"));
    assert!(hold.contains("route claude-api-ming is not billed to its API key"));
    crate::cmd::config::run(
        store,
        crate::cli::ConfigAction::ClearLimit {
            agent: "claude-api-ming".into(),
        },
    )
    .unwrap();
    next_launch(home, store).unwrap();
    assert_eq!(sweep_count(store), 1);
}

#[test]
fn stopped_route_before_init_holds_next_launch() {
    let (home, _guard, store) = fixture();
    let (_, _, _, _isolated) = prepared(home.path(), &store, &spec(false));
    next_launch(home.path(), &store).unwrap();
    assert_eq!(sweep_count(&store), 0);
    crate::cmd::stop::stop(&store, "t-api-launch").unwrap();
    assert_eq!(
        store.get_task("t-api-launch").unwrap().unwrap().status,
        crate::types::TaskStatus::Stopped
    );
    assert_next_launch_held_then_cleared(home.path(), &store);
}

#[test]
fn dead_route_worker_without_init_holds_next_launch() {
    let (home, _guard, store) = fixture();
    let mut spec = spec(false);
    let (_, _, _, _isolated) = prepared(home.path(), &store, &spec);
    spec.worker_pid = Some(999_999);
    crate::runtime::background::save_spec(&spec).unwrap();
    let cleaned = crate::runtime::background::check_zombie_tasks_with(&store, |_| false).unwrap();
    assert!(cleaned.iter().any(|id| id == "t-api-launch"));
    assert_eq!(
        store.get_task("t-api-launch").unwrap().unwrap().status,
        crate::types::TaskStatus::Failed
    );
    assert_next_launch_held_then_cleared(home.path(), &store);
}

#[tokio::test]
async fn verified_route_task_does_not_hold_next_launch() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store) = fixture();
    let spec = spec(false);
    let (args, agent, command, _isolated) = prepared(home.path(), &store, &spec);
    crate::runtime::background::run_process(
        &store, &spec, &args, agent.as_ref(), command, std::time::Instant::now(),
    ).await.unwrap();
    assert_eq!(
        store.get_task("t-api-launch").unwrap().unwrap().status,
        crate::types::TaskStatus::Done
    );
    next_launch(home.path(), &store).unwrap();
    assert_eq!(sweep_count(&store), 0);
}

#[test]
fn swept_task_is_counted_once_after_clear() {
    let (home, _guard, store) = fixture();
    let (_, _, _, _isolated) = prepared(home.path(), &store, &spec(false));
    crate::cmd::stop::stop(&store, "t-api-launch").unwrap();
    assert_next_launch_held_then_cleared(home.path(), &store);
    next_launch(home.path(), &store).unwrap();
    assert_eq!(sweep_count(&store), 1);
}

#[test]
fn renamed_route_task_without_init_holds_next_launch() {
    let (home, _guard, store) = fixture();
    let (_, _, _, _isolated) = prepared(home.path(), &store, &spec(false));
    crate::cmd::stop::stop(&store, "t-api-launch").unwrap();
    store
        .db()
        .execute("UPDATE tasks SET custom_agent_name = 'other' WHERE id = 't-api-launch'", [])
        .unwrap();
    assert_next_launch_held_then_cleared(home.path(), &store);
}

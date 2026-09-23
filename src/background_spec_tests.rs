// Upgrade regression tests for persisted background specs.
// Covers loading an unchanged pre-upgrade job fixture.
// Deps: background spec loader, isolated AID_HOME, and fixture JSON.

use super::load_spec_if_exists;
use crate::paths;

#[test]
fn pid_updates_publish_complete_specs_to_concurrent_readers() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(temp.path());
    let mut spec: super::BackgroundRunSpec = serde_json::from_str(
        include_str!("../testdata/legacy-background-spec.json")).unwrap();
    super::save_spec(&spec).unwrap();
    std::thread::scope(|scope| {
        let home = temp.path();
        scope.spawn(move || {
            let _guard = paths::AidHomeGuard::set(home);
            for _ in 0..500 {
                assert!(load_spec_if_exists("t-9ef43f87").unwrap().is_some());
            }
        });
        for pid in 1..100 {
            spec.worker_pid = Some(pid);
            super::save_spec(&spec).unwrap();
        }
    });
    assert_eq!(load_spec_if_exists(&spec.task_id).unwrap().unwrap().worker_pid, Some(99));
    assert_eq!(std::fs::read_dir(paths::jobs_dir()).unwrap().count(), 1);
}

#[test]
fn absent_or_removed_spec_is_settled_but_invalid_spec_is_an_error() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(temp.path());
    assert!(load_spec_if_exists("t-spec").unwrap().is_none());
    paths::ensure_dirs().unwrap();
    let path = paths::job_path("t-spec");
    std::fs::write(&path, "invalid json").unwrap();
    assert!(load_spec_if_exists("t-spec").is_err());
    std::fs::remove_file(path).unwrap();
    assert!(load_spec_if_exists("t-spec").unwrap().is_none());
}

#[test]
fn actual_pre_upgrade_spec_from_jobs_loads_without_detached_field() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let content = include_str!("../testdata/legacy-background-spec.json");
    assert!(content.contains("\"detached\": true"));
    std::fs::write(paths::job_path("t-9ef43f87"), content).unwrap();

    let spec = load_spec_if_exists("t-9ef43f87").unwrap().unwrap();
    assert_eq!(spec.task_id, "t-9ef43f87");
    assert_eq!(spec.agent_name, "grok");
    assert!(spec.interactive);
}

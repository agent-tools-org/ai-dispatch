// Upgrade regression tests for persisted background specs.
// Covers loading an unchanged pre-upgrade job fixture.
// Deps: background spec loader, isolated AID_HOME, and fixture JSON.

use super::load_spec_if_exists;
use crate::paths;

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

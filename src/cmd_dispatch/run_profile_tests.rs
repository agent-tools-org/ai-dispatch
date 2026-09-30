// Isolated AID_HOME coverage for explicit_agent egress gates.
// Exports: (tests only)
// Deps: explicit_agent, AidHomeGuard, TaskEgress.

use super::*;
use crate::paths::AidHomeGuard;
use crate::types::TaskEgress;

fn isolated_home() -> (tempfile::TempDir, AidHomeGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = AidHomeGuard::set(temp.path());
    (temp, guard)
}

#[test]
fn run_resolution_keeps_explicit_claude_with_missing_profile_or_critical_rigor() {
    let (_temp, _guard) = isolated_home();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![crate::types::AgentKind::Claude]);
    let store = store::Store::open_memory().expect("store");
    for rigor in [None, Some(TaskRigor::Critical)] {
        let result = resolve_run_agent(&store, "refactor the scheduler", None, None, None,
            rigor, TaskEgress::Any, None, false, &None, "claude".into());
        assert_eq!(result.expect("explicit dispatch"), "claude");
    }
}

#[test]
fn egress_local_refuses_builtin_third_party() {
    let (_temp, _guard) = isolated_home();
    let err = explicit_agent("codex".into(), TaskEgress::Local)
        .expect_err("codex must fail --egress local");
    assert!(err.to_string().contains("--egress local"));
}

#[test]
fn egress_any_admits_third_party() {
    let (_temp, _guard) = isolated_home();
    assert!(explicit_agent("codex".into(), TaskEgress::Any).is_ok());
}

#[test]
fn egress_private_network_refuses_public_third_party() {
    let (_temp, _guard) = isolated_home();
    let err = explicit_agent("codex".into(), TaskEgress::PrivateNetwork)
        .expect_err("codex must fail --egress private-network");
    assert!(err.to_string().contains("--egress private-network"));
}

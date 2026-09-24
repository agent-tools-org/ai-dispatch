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
fn rigor_no_longer_gates_agent_identity() {
    let (_temp, _guard) = isolated_home();
    let result = explicit_agent("claude".into(), TaskEgress::Any);
    assert!(result.is_ok(), "identity gate must be gone: {result:?}");
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

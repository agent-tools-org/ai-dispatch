// Settlement diagnostics for incompatible cross-auditor binaries.
// Uses the sibling audit fixtures, shared shim and in-memory store.

use super::*;

#[test]
fn settlement_explains_incompatible_binaries_without_changing_verdict_policy() {
    let _guard = env_lock();
    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("aic");
    set_env("AIC_TEST_BINARY", &binary);
    for (version, help, expected) in [
        ("aic 0.3.1", "AI-powered commit message generator", "Foreign"),
        ("aic 0.3.0", "Cross-audit aid agent outputs", "TooOld(aic 0.3.0)"),
    ] {
        crate::support::test_env::write_executable(
            &binary, &aic_shim::identified_script(version, help, "exit 0"),
        );
        for critical in [false, true] {
            let store = Store::open_memory().unwrap();
            let mut pending = done_task("t-identity");
            if critical { pending.completed_at = None; }
            store.insert_task(&pending).unwrap();
            let mut args = audit_args(true);
            if critical { args.declared_rigor = Some(crate::types::TaskRigor::Critical); }
            run_audit_for_task(&store, "t-identity", &args);
            let task = store.get_task("t-identity").unwrap().unwrap();
            assert_eq!(task.audit_verdict.as_deref(), Some(if critical { "error" } else { "skipped" }));
            assert_eq!(task.status, if critical { TaskStatus::Failed } else { TaskStatus::Done });
            let events = store.get_events("t-identity").unwrap();
            let event = events.iter().find(|event| event.detail.contains(expected)).unwrap();
            assert!(event.detail.contains("curl -fsSL https://aid.agent-tools.org/install.sh | sh"));
            if expected == "Foreign" {
                assert!(event.detail.contains("cargo install aic installs an unrelated project"));
            }
        }
    }
    remove_env("AIC_TEST_BINARY");
}

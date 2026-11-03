// Reaper identity regressions across target capture and both signal passes.
// Uses real process groups with injected stale launch identities.
use super::tests::{spawn_tree, spec};
use super::*;

#[test]
fn process_safety_reaper_rechecks_captured_worker_and_descendant_identities() {
    let _permit = crate::support::test_subprocess::acquire();
    let (mut parent, child) = spawn_tree(&["__run-task", "t-kill"]);
    signal_targets_with(Some(parent.id()), &spec(None), |targets| {
        assert!(targets.iter().any(|(pid, _)| *pid == parent.id()));
        assert!(targets.iter().any(|(pid, _)| *pid == child as u32));
        for (_, started) in targets {
            *started = Some("recycled-launch".into());
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    let survived = parent.try_wait().expect("wait").is_none()
        && crate::support::test_subprocess::is_live(child);
    unsafe {
        libc::kill(child, libc::SIGKILL);
    }
    let _ = parent.kill();
    let _ = parent.wait();
    assert!(
        survived,
        "stale targets must not signal a recycled process group"
    );
}

// Default text stop retains main's process probe, including its zombie handling.
// Caller-owned exit confirmation remains covered by the separate non-reaping regressions.
use super::*;

#[cfg(unix)]
#[test]
fn default_text_stop_uses_mains_zombie_reaping_probe() {
    let _permit = crate::support::test_subprocess::acquire();
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 7"])
        .spawn()
        .expect("child");
    assert!(crate::support::test_subprocess::wait_dead(
        child.id() as i32,
        Duration::from_secs(5)
    ));
    assert!(!wait_for_exit(child.id()));
    assert_eq!(
        child
            .wait()
            .expect_err("main probe already reaped the zombie")
            .raw_os_error(),
        Some(libc::ECHILD)
    );
}

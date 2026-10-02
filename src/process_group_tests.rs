// Tests for process-group termination: escalation, descendant walk across
// sessions, and start-time identity before the SIGKILL pass.
// Deps: process_group internals, test_subprocess, sh and perl.

use super::{descendants_in_table, kill_with_grace, parse_table, unchanged_pids};
use crate::test_subprocess::{self, is_live};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;
use std::time::Duration;

#[test]
fn kill_with_grace_escalates_for_term_ignoring_child() {
    let _permit = test_subprocess::acquire();
    let ready = tempfile::NamedTempFile::new().expect("ready file should be created");
    let ready_path = ready.path().to_path_buf();
    drop(ready);
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("trap '' TERM; touch \"$1\"; while :; do sleep 1; done")
        .arg("sh")
        .arg(&ready_path)
        .process_group(0);
    let mut child = command.spawn().expect("TERM-ignoring child should spawn");
    let pgid = child.id() as i32;
    for _ in 0..100 {
        if ready_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready_path.exists());

    kill_with_grace(pgid, Duration::from_millis(100));

    let status = child.wait().expect("TERM-ignoring child should exit");
    assert_eq!(status.signal(), Some(libc::SIGKILL));
}

#[test]
fn descendants_follow_ppid_chain_and_skip_unrelated_and_init() {
    let table = parse_table(
        &[
            " 10 1 Sat Oct  3 00:00:01 2026",
            " 11 10 Sat Oct  3 00:00:02 2026",
            " 12 11 Sat Oct  3 00:00:03 2026",
            " 13 12 Sat Oct  3 00:00:04 2026",
            " 20 1 Sat Oct  3 00:00:05 2026",
            " 21 20 Sat Oct  3 00:00:06 2026",
            " bad row",
            " 30 10",
        ]
        .join("\n"),
    );

    let mut found = descendants_in_table(10, &table)
        .iter()
        .map(|e| e.pid)
        .collect::<Vec<_>>();
    found.sort_unstable();

    assert_eq!(found, vec![11, 12, 13]);
    let init = parse_table(" 1 0 Sat Oct  3 00:00:00 2026\n 2 1 Sat Oct  3 00:00:00 2026");
    assert!(
        descendants_in_table(1, &init)
            .iter()
            .all(|entry| entry.pid != 1)
    );
}

/// A descendant that exited during the grace period and whose pid now belongs to a
/// newer process (different start time), or that `ps` no longer lists, is not SIGKILLed.
#[test]
fn sigkill_pass_skips_descendants_whose_pid_was_reused_or_is_unverifiable() {
    let snapshot = parse_table(" 11 10 Sat Oct  3 00:00:02 2026\n 12 11 Sat Oct  3 00:00:03 2026");
    let now = parse_table(" 11 1 Sat Oct  3 00:00:02 2026\n 12 1 Sat Oct  3 00:09:59 2026");

    assert_eq!(unchanged_pids(&snapshot, &now), vec![11]);
    assert!(unchanged_pids(&snapshot, &[]).is_empty());
}

/// A tool command started in its own process group (Claude Code's Bash runs each
/// command in a new session) survived the agent's group kill before this fix.
#[test]
fn kill_with_grace_reaches_descendant_in_its_own_group() {
    assert_own_group_descendant_killed("perl -e 'setpgrp(0, 0); exec q(sleep), q(60)'");
}

/// Only the start-time-verified SIGKILL pass can stop a TERM-ignoring descendant,
/// so this proves real `ps` lstart rows match across the grace period.
#[test]
fn kill_with_grace_sigkills_term_ignoring_descendant_after_identity_check() {
    assert_own_group_descendant_killed("perl -e '$SIG{TERM} = q(IGNORE); setpgrp(0, 0); sleep 60'");
}

fn assert_own_group_descendant_killed(descendant: &str) {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().expect("temp dir");
    let pid_file = dir.path().join("grandchild.pid");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(format!("{descendant} & echo $! > \"$1\"; wait"))
        .arg("sh")
        .arg(&pid_file)
        .process_group(0);
    let mut child = command.spawn().expect("agent stand-in should spawn");
    let mut grandchild = 0;
    for _ in 0..200 {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
            .filter(|pid| unsafe { libc::getpgid(*pid) } == *pid)
        {
            grandchild = pid;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(grandchild > 0, "grandchild must lead its own process group");

    kill_with_grace(child.id() as i32, Duration::from_millis(200));
    let _ = child.wait();

    let gone = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        !is_live(grandchild)
    });
    if !gone {
        unsafe { libc::kill(grandchild, libc::SIGKILL) };
    }
    assert!(
        gone,
        "descendant in its own process group must be killed with the agent"
    );
}

// Cross-platform process sample and progress classification regressions.
// Covers Linux tick layout, macOS CPU precision, PID reuse and remote-wait ownership.
// Deps: shared activity probe and synthetic process snapshots.

use super::*;

fn process(pid: u32, ppid: u32, cpu: u64, remote_wait: bool) -> Process {
    Process {
        pid,
        ppid,
        group: 100,
        cpu,
        remote_wait,
        started: "start".into(),
    }
}

fn sample(children: Vec<Process>, time: SystemTime) -> Sample {
    Sample {
        root: 100,
        started: "root".into(),
        sampled_at: time,
        progress_at: time,
        children,
    }
}

#[test]
fn macos_fractional_cpu_time_preserves_short_window_progress() {
    assert_eq!(cpu_time("0:00.02"), Some(20_000));
    assert_eq!(cpu_time("12:34.56"), Some(754_560_000));
    assert_eq!(cpu_time("1-02:03:04"), Some(93_784_000_000));
}

#[test]
fn linux_proc_ticks_and_start_time_ignore_spaces_and_parentheses_in_name() {
    let stat = "42 (tool (name)) S 1 2 3 4 5 6 7 8 9 10 123 456 0 0 0 0 1 0 789 0";
    assert_eq!(proc_cpu(stat), Some((579, "789")));
    assert!(proc_cpu("broken").is_none());
}

#[test]
fn descendant_cpu_growth_counts_but_existence_does_not() {
    let start = SystemTime::UNIX_EPOCH;
    let now = start + Duration::from_secs(2);
    let previous = sample(vec![process(101, 100, 1, false)], start);
    assert_eq!(
        advance(Some(&previous), &[process(101, 100, 2, false)], 100, now),
        now
    );
    assert_eq!(
        advance(Some(&previous), &[process(101, 100, 1, false)], 100, now),
        start
    );
}

#[test]
fn remote_wait_requires_agent_group_ownership() {
    let start = SystemTime::UNIX_EPOCH;
    let now = start + Duration::from_secs(2);
    let mut child = process(101, 100, 0, true);
    let previous = sample(vec![child.clone()], start);
    assert_eq!(advance(Some(&previous), &[child.clone()], 100, now), now);
    child.group = 200;
    assert_eq!(advance(Some(&previous), &[child], 100, now), start);
}

#[test]
fn known_remote_waits_are_distinguished_from_sleeping_shells() {
    for args in [
        vec!["ssh", "box", "build"],
        vec!["python3", "/bin/rbox", "exec", "box"],
        vec!["/bin/sh", "./rbox", "task", "--wait"],
    ] {
        assert!(is_remote_wait(&args), "{args:?}");
    }
    for args in [
        vec!["sleep", "600"],
        vec!["sh", "-c", "ssh box build"],
        vec!["rbox", "status"],
    ] {
        assert!(!is_remote_wait(&args), "{args:?}");
    }
}

#[test]
fn descendants_include_nested_tools_and_exclude_unrelated_group_members() {
    let table = vec![
        process(101, 100, 0, false),
        process(102, 101, 0, false),
        process(103, 999, 0, true),
    ];
    assert_eq!(
        descendants(100, &table)
            .iter()
            .map(|p| p.pid)
            .collect::<Vec<_>>(),
        vec![101, 102]
    );
}

#[test]
fn reused_pid_starts_a_new_cpu_observation() {
    let start = SystemTime::UNIX_EPOCH;
    let now = start + Duration::from_secs(2);
    let previous = sample(vec![process(101, 100, 500, false)], start);
    let mut child = process(101, 100, 1, false);
    child.started = "replacement".into();
    assert_eq!(advance(Some(&previous), &[child], 100, now), now);
}

#[test]
fn unwritable_sample_does_not_claim_child_progress() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("child-activity.json");
    std::fs::create_dir(&path).expect("blocked sample path");
    assert!(persist(&path, &sample(vec![], SystemTime::UNIX_EPOCH)).is_none());
}

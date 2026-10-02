// Remote verification deadline regressions using saved policies and fake commands.
// Covers remaining budgets, honest expiry, re-pick persistence and local fallback.
// Deps: sibling fake fixtures, isolated AID home, chrono and in-memory Store.
use super::*;
use super::{shim_tests::executable, tests::{refusing_verify, stored_task}};
use chrono::{DateTime, Local};
use std::time::Duration;

fn save_policy(store: &Store, id: &TaskId, max_duration: Duration, hard_cap: Duration) {
    let mut args = RunArgs::saved_for_task(store, id.as_str()).expect("saved").expect("args");
    args.timeout_policy.max_duration = max_duration;
    args.timeout_policy.hard_cap = hard_cap;
    // The resolved policy, rather than this older CLI value, owns the deadline.
    args.max_duration_mins = Some(1);
    store.update_task_dispatch_args(id.as_str(), &args.dispatch_args_json().expect("args"))
        .expect("save policy");
}

fn timestamps(store: &Store, id: &TaskId, created: DateTime<Local>, started: Option<DateTime<Local>>) {
    store.db().execute("UPDATE tasks SET created_at = ?1, started_at = ?2 WHERE id = ?3",
        rusqlite::params![created.to_rfc3339(), started.map(|time| time.to_rfc3339()), id.as_str()])
        .expect("timestamps");
}

fn isolated_runner(test: &str) -> bool {
    if std::env::var("AID_DEADLINE_TEST_CHILD").as_deref() == Ok(test) { return true; }
    let temp = tempfile::tempdir().expect("child home");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", &format!("remote_build::deadline_tests::{test}"), "--nocapture"])
        .env("AID_DEADLINE_TEST_CHILD", test).env("AID_HOME", temp.path())
        .output().expect("isolated runner");
    assert!(output.status.success(), "{output:?}");
    false
}

#[test]
fn configured_remote_allowance_above_120_seconds_reaches_verify_runner() {
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let now = Local::now();
    save_policy(&store, &id, Duration::from_secs(900), Duration::from_secs(1800));
    timestamps(&store, &id, now, Some(now));
    let deadline = verify_deadline(&store, id.as_str(), now).expect("deadline");
    // Deterministic evidence for the very timeout passed by verify_on to the runner.
    assert_eq!(remaining_verify_time(deadline, now), Duration::from_secs(900));
    assert!(remaining_verify_time(deadline, now) > crate::verify::VERIFY_TIMEOUT);
    executable(&temp.path().join("verify"), "#!/bin/bash\nprintf '%s' \"$AID_BUILD_BOX\"\n");
    let result = verify_on(id.as_str(), "chosen-box", temp.path(), Some("./verify"), None, None, deadline)
        .expect("runner");
    assert!(result.success, "{}", result.output);
    assert!(!result.timed_out);
    assert_eq!(result.output, "chosen-box");
}

#[test]
fn elapsed_started_time_reduces_budget_and_unstarted_uses_creation_time() {
    let (store, id, _) = stored_task();
    let now = Local::now();
    save_policy(&store, &id, Duration::from_secs(600), Duration::from_secs(1200));
    let created = now - chrono::Duration::seconds(400);
    timestamps(&store, &id, created, Some(now - chrono::Duration::seconds(300)));
    let deadline = verify_deadline(&store, id.as_str(), now).expect("started deadline");
    assert_eq!(remaining_verify_time(deadline, now), Duration::from_secs(300));
    // A re-pick milestone must not substitute for the missing started_at column.
    record(&store, &id, &RunArgs { remote_build: Some("second-box".into()), ..Default::default() })
        .expect("milestone");
    timestamps(&store, &id, created, None);
    let deadline = verify_deadline(&store, id.as_str(), now).expect("unstarted deadline");
    assert_eq!(remaining_verify_time(deadline, now), Duration::from_secs(200));
}

#[test]
fn sub_120_second_max_duration_and_hard_cap_are_both_respected() {
    let (store, id, _) = stored_task();
    let now = Local::now();
    timestamps(&store, &id, now, Some(now));
    for (max, hard) in [(30, 900), (900, 30)] {
        save_policy(&store, &id, Duration::from_secs(max), Duration::from_secs(hard));
        let deadline = verify_deadline(&store, id.as_str(), now).expect("deadline");
        assert_eq!(remaining_verify_time(deadline, now), Duration::from_secs(30));
    }
}

#[test]
fn short_remote_cap_kills_the_command_and_remains_inconclusive() {
    if !isolated_runner("short_remote_cap_kills_the_command_and_remains_inconclusive") { return; }
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(&temp.path().join("verify"), "#!/bin/bash\necho launched > launched\nexec /bin/sleep 5\n");
    save_policy(&store, &id, Duration::from_secs(900), Duration::from_millis(500));
    let now = Local::now();
    timestamps(&store, &id, now, Some(now));
    let result = verify(&store, id.as_str(), temp.path(), Some("./verify"), None, None).expect("verify");
    assert!(temp.path().join("launched").is_file());
    assert!(result.timed_out, "{}", result.output);
    assert!(!result.success);
    assert!(!result.infrastructure_failure);
    assert_eq!(result.exit_code, None);
    assert!(result.output.contains("Verification timed out"));
}

#[test]
fn exhausted_task_deadline_launches_neither_verify_nor_remote_pick() {
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(&temp.path().join("verify"), "#!/bin/bash\necho launched > launched\nexit 69\n");
    let rbox = temp.path().join("rbox");
    executable(&rbox, "#!/bin/bash\necho picked > picked\necho second-box\n");
    save_policy(&store, &id, Duration::from_secs(30), Duration::from_secs(900));
    timestamps(&store, &id, Local::now() - chrono::Duration::seconds(31), None);
    let mut picker = Command::new(&rbox);
    picker.current_dir(temp.path());
    let result = verify_with(&store, id.as_str(), temp.path(), Some("./verify"), None, None,
        &mut picker).expect("expired");
    assert!(result.timed_out);
    assert!(!result.success);
    assert!(!result.infrastructure_failure);
    assert_eq!(result.exit_code, None);
    assert_eq!(result.command, "./verify");
    assert!(result.output.contains("no command launched"));
    assert!(!temp.path().join("launched").exists());
    assert!(!temp.path().join("picked").exists());
    crate::verify::record_verify_status(&store, &id, &result);
    let task = store.get_task(id.as_str()).expect("task").expect("exists");
    assert_eq!(task.verify_status, crate::types::VerifyStatus::TimedOut);
    assert_eq!(task.status, crate::types::TaskStatus::Pending);
}

#[test]
fn near_future_timestamp_is_clamped_once_and_never_extends_the_cap() {
    let (store, id, _) = stored_task();
    let now = Local::now();
    save_policy(&store, &id, Duration::from_secs(900), Duration::from_secs(300));
    for started in [None, Some(now + chrono::Duration::seconds(1))] {
        timestamps(&store, &id, now + chrono::Duration::seconds(1), started);
        let deadline = verify_deadline(&store, id.as_str(), now).expect("deadline");
        assert_eq!(remaining_verify_time(deadline, now), Duration::from_secs(300));
        assert_eq!(remaining_verify_time(deadline, now + chrono::Duration::seconds(100)),
            Duration::from_secs(200));
        assert_eq!(remaining_verify_time(deadline, now + chrono::Duration::seconds(301)),
            Duration::ZERO);
    }
}

#[test]
fn repick_consumes_original_deadline_and_persists_the_attempted_replacement() {
    if !isolated_runner("repick_consumes_original_deadline_and_persists_the_attempted_replacement") { return; }
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(&temp.path().join("verify"), "#!/bin/bash\necho \"$AID_BUILD_BOX\" >> attempts\nif [[ $AID_BUILD_BOX == chosen-box ]]; then\n  /bin/sleep 0.5\n  echo 'rbox: disk admission refused' >&2\n  exit 69\nfi\nexec /bin/sleep 1.5\n");
    let rbox = temp.path().join("rbox");
    executable(&rbox, "#!/bin/bash\n/bin/sleep 0.5\necho second-box\n");
    save_policy(&store, &id, Duration::from_secs(2), Duration::from_secs(900));
    let now = Local::now();
    timestamps(&store, &id, now, Some(now));
    let result = verify_with(&store, id.as_str(), temp.path(), Some("./verify"), None, None,
        &mut Command::new(&rbox)).expect("verify");
    // A reset two-second budget would allow the replacement's 1.5-second command to finish.
    assert!(result.timed_out, "{}", result.output);
    assert!(!result.success);
    assert_eq!(std::fs::read_to_string(temp.path().join("attempts")).expect("attempts"),
        "chosen-box\nsecond-box\n");
    assert_eq!(saved_box(&store, id.as_str()).expect("box").as_deref(), Some("second-box"));
    let events = store.get_events(id.as_str()).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].metadata.as_ref().expect("metadata")["remote_build"], "second-box");
}

#[test]
fn deadline_exhausted_during_repick_skips_replacement_command() {
    if !isolated_runner("deadline_exhausted_during_repick_skips_replacement_command") { return; }
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    refusing_verify(temp.path(), "chosen-box");
    let rbox = temp.path().join("rbox");
    executable(&rbox, "#!/bin/bash\n/bin/sleep 1\necho second-box\n");
    save_policy(&store, &id, Duration::from_millis(500), Duration::from_secs(900));
    let now = Local::now();
    timestamps(&store, &id, now, Some(now));
    let result = verify_with(&store, id.as_str(), temp.path(), Some("./verify"), None, None,
        &mut Command::new(&rbox)).expect("expired after pick");
    assert!(result.timed_out);
    assert_eq!(result.exit_code, None);
    assert!(result.output.contains("no command launched"));
    assert_eq!(saved_box(&store, id.as_str()).expect("box").as_deref(), Some("second-box"));
}

#[test]
fn local_and_legacy_verification_ignore_task_deadline_and_keep_120_seconds() {
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(&temp.path().join("verify"), "#!/bin/bash\n/bin/sleep 0.1\necho local\n");
    timestamps(&store, &id, Local::now() - chrono::Duration::hours(2), None);
    let args = RunArgs { timeout_policy: crate::timeout_policy::TimeoutPolicy {
        max_duration: Duration::from_millis(1), ..Default::default()
    }, ..Default::default() };
    store.update_task_dispatch_args(id.as_str(), &args.dispatch_args_json().expect("args"))
        .expect("local args");
    for legacy in [false, true] {
        if legacy {
            store.db().execute("UPDATE tasks SET dispatch_args = NULL WHERE id = ?1", [id.as_str()])
                .expect("legacy task");
        }
        let result = verify(&store, id.as_str(), temp.path(), Some("./verify"), None, None)
            .expect("local verify");
        assert!(result.success, "{}", result.output);
        assert!(!result.timed_out);
        assert_eq!(result.output, "local\n");
    }
    assert_eq!(crate::verify::VERIFY_TIMEOUT, Duration::from_secs(120));
}

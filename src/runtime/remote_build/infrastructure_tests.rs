// Remote verify failover regressions with a fake rbox, never live box contact.
// Covers persisted auto provenance, named pins, one replacement, and no replay.
use super::tests::stored_task;
use super::*;
use crate::support::test_env::write_executable;

fn fake_rbox(dir: &Path, first: &str, second: &str) -> (std::path::PathBuf, String) {
    let rbox = dir.join("rbox");
    write_executable(
        &rbox,
        &format!(
            r#"#!/bin/bash
printf '%s\n' "$*" >> '{}'
if [[ $1 == pick ]]; then echo second-box; exit 0; fi
if [[ $2 == chosen-box ]]; then
{first}
else
{second}
fi
"#,
            dir.join("calls").display()
        ),
    );
    let command = format!(
        "PATH='{}:/usr/bin:/bin' rbox exec \"$AID_BUILD_BOX\"",
        dir.display()
    );
    (rbox, command)
}

#[test]
fn auto_provenance_is_saved_before_resolution_and_legacy_names_are_pinned() {
    let temp = tempfile::tempdir().expect("temp");
    let (rbox, _) = fake_rbox(temp.path(), "exit 1", "exit 1");
    let mut args = RunArgs {
        remote_build: Some("auto".into()),
        ..Default::default()
    };
    resolve_in(&mut args, true, &mut Command::new(&rbox)).expect("resolve");
    assert_eq!(args.remote_build.as_deref(), Some("second-box"));
    assert!(args.remote_build_auto_picked);
    let restored =
        RunArgs::from_dispatch_args_json(&args.dispatch_args_json().expect("json")).expect("restore");
    assert!(restored.remote_build_auto_picked);
    let legacy = RunArgs::from_dispatch_args_json(r#"{"remote_build":"chosen-box"}"#).expect("legacy");
    assert!(!legacy.remote_build_auto_picked);
    let mut named = RunArgs {
        remote_build: Some("named-box".into()),
        ..Default::default()
    };
    resolve_in(&mut named, true, &mut Command::new("must-not-run")).expect("named");
    assert!(!named.remote_build_auto_picked);
}

#[test]
fn offline_auto_box_retries_once_but_named_box_does_not_switch() {
    for auto in [true, false] {
        let (store, id, mut args) = stored_task();
        args.remote_build_auto_picked = auto;
        store
            .update_task_dispatch_args(id.as_str(), &args.dispatch_args_json().expect("json"))
            .expect("save");
        let temp = crate::support::test_env::TestHome::new();
        let (rbox, command) = fake_rbox(
            temp.path(),
            "echo 'rbox: tailnet-offline (probe age 0.0s): chosen-box: offline'; exit 1",
            "echo 'rbox: job abc exited with code 0'; exit 0",
        );
        let result = verify_with(
            &store,
            id.as_str(),
            temp.path(),
            Some(&command),
            None,
            None,
            &mut Command::new(&rbox),
        )
        .expect("verify");
        let calls = std::fs::read_to_string(temp.path().join("calls")).expect("calls");
        assert_eq!(calls.lines().count(), if auto { 3 } else { 1 });
        assert_eq!(result.success, auto);
        assert_eq!(result.infrastructure_failure, !auto);
        assert_eq!(
            saved_box(&store, id.as_str()).expect("box").as_deref(),
            Some(if auto { "second-box" } else { "chosen-box" })
        );
        if auto {
            assert!(calls.contains("--exclude chosen-box"), "{calls}");
        }
    }
}

#[test]
fn second_infrastructure_failure_stops_after_one_replacement() {
    let (store, id, _) = stored_task();
    let temp = crate::support::test_env::TestHome::new();
    let (rbox, command) = fake_rbox(
        temp.path(),
        "echo 'rbox: Box lock not acquired within 60 s'; exit 75",
        "echo 'rbox: second-box: expected stamp abc, actual missing; run rbox ensure explicitly'; exit 1",
    );
    let result = verify_with(
        &store,
        id.as_str(),
        temp.path(),
        Some(&command),
        None,
        None,
        &mut Command::new(&rbox),
    )
    .expect("verify");
    assert!(result.infrastructure_failure);
    assert!(result.output.contains("second-box: expected stamp"));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("calls"))
            .expect("calls")
            .lines()
            .count(),
        3
    );
}

#[test]
fn completed_numeric_exits_sync_failure_and_running_jobs_never_repick() {
    for output in [
        "echo 'rbox: job abc exited with code 69'; exit 69",
        "echo compiler-error; exit 69",
        "echo 'rbox: sync to work/repo (bundle 12 bytes): missing input'; exit 1",
        "echo 'rbox: job abc still running after 10 s; the job continues on the box'; exit 124",
    ] {
        let (store, id, _) = stored_task();
        let temp = crate::support::test_env::TestHome::new();
        let (rbox, command) = fake_rbox(temp.path(), output, "exit 0");
        let result = verify_with(
            &store,
            id.as_str(),
            temp.path(),
            Some(&command),
            None,
            None,
            &mut Command::new(&rbox),
        )
        .expect("verify");
        assert!(!result.success);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("calls"))
                .expect("calls")
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn unusable_replacement_never_falls_back_to_local_or_reuses_failed_host() {
    for response in ["exit 1", "echo chosen-box", "exit 0"] {
        let (store, id, _) = stored_task();
        let temp = crate::support::test_env::TestHome::new();
        let (rbox, command) = fake_rbox(
            temp.path(),
            "echo 'rbox: Box lock not acquired within 60 s'; exit 75",
            "exit 0",
        );
        let picker = temp.path().join("picker");
        write_executable(&picker, &format!("#!/bin/sh\n{response}\n"));
        let error = verify_with(
            &store,
            id.as_str(),
            temp.path(),
            Some(&command),
            None,
            None,
            &mut Command::new(&picker),
        )
        .expect_err("no usable replacement");
        assert!(error.to_string().contains("re-pick failed"));
        assert_eq!(
            saved_box(&store, id.as_str()).expect("box").as_deref(),
            Some("chosen-box")
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("calls"))
                .expect("calls")
                .lines()
                .count(),
            1
        );
        assert!(rbox.is_file());
    }
}

#[test]
fn typed_repick_runs_once_only_when_every_receipt_is_retryable() {
    for reason in ["lock_timeout", "sync_failed"] {
        let (store, id, _) = stored_task();
        let temp = crate::support::test_env::TestHome::new();
        let refusal = serde_json::json!({"version":1, "phase":"not_started", "job":null,
            "reason":"lock_timeout", "detail":"typed refusal"});
        let mut other = refusal.clone();
        other["reason"] = serde_json::json!(reason);
        let body = format!(r#"printf '%s\n' '{refusal}' '{other}' >> "$AID_RBOX_RESULT"; exit 1"#);
        let (rbox, command) = fake_rbox(temp.path(), &body, &body);
        let result = verify_with(&store, id.as_str(), temp.path(), Some(&command), None, None,
            &mut Command::new(&rbox)).expect("verify");
        assert!(!result.success);
        assert!(result.infrastructure_failure);
        assert!(result.rbox_result.is_some());
        let calls = std::fs::read_to_string(temp.path().join("calls")).expect("calls");
        let repicked = reason == "lock_timeout";
        let picks = calls.lines().filter(|line| line.starts_with("pick ")).count();
        assert_eq!(picks, usize::from(repicked));
        assert_eq!(calls.lines().count(), if repicked { 3 } else { 1 });
        assert_eq!(saved_box(&store, id.as_str()).expect("box").as_deref(),
            Some(if repicked { "second-box" } else { "chosen-box" }));
    }
}

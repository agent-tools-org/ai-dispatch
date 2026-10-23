// Production stream watchdog and deadline parity regressions.
// Uses interactive specs with deterministic usage and liveness streams.
use super::*;

fn policy(args: &mut RunArgs, values: &[(&str, &str)]) {
    args.env = Some(
        values
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    );
}

#[tokio::test]
async fn deadline_preserves_partial_transcript_and_pid_for_stop() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, mut args) = setup();
    policy(&mut args, &[("AID_MAX_DURATION_SECS", "30")]);
    tokio::time::timeout(
        Duration::from_secs(120),
        run_process(
            &store,
            &spec,
            &args,
            &ClaudeAgent,
            gated_tests::gated_command("partial", &home.path().join("input")),
            std::time::Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status(&store), TaskStatus::Failed);
    let transcript = std::fs::read_to_string(paths::transcript_path(&spec.task_id)).unwrap();
    assert!(transcript.contains("partial raw output"));
    assert!(
        store
            .get_events(&spec.task_id)
            .unwrap()
            .iter()
            .any(|event| event.detail.contains("exceeded hard timeout of 30 seconds"))
    );
    assert!(
        load_spec_if_exists(&spec.task_id)
            .unwrap()
            .unwrap()
            .agent_pid
            .is_some()
    );
}

#[tokio::test]
async fn first_token_and_idle_timeouts_remain_distinct_on_interactive_specs() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, mut args) = setup();
    for (mode, first, idle, transient) in [
        ("startup", "30", "3600", true),
        ("idle", "3600", "30", false),
    ] {
        store
            .db()
            .execute(
                "UPDATE tasks SET status='running' WHERE id='t-production'",
                [],
            )
            .unwrap();
        policy(
            &mut args,
            &[
                ("AID_FIRST_TOKEN_TIMEOUT_SECS", first),
                ("AID_IDLE_TIMEOUT_SECS", idle),
            ],
        );
        tokio::time::timeout(
            Duration::from_secs(120),
            run_process(
                &store,
                &spec,
                &args,
                &ClaudeAgent,
                gated_tests::gated_command(mode, &home.path().join("input")),
                std::time::Instant::now(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(status(&store), TaskStatus::Failed);
        let transcript = std::fs::read_to_string(paths::transcript_path(&spec.task_id)).unwrap();
        assert_eq!(transcript.contains("working"), mode == "idle");
        assert!(
            store
                .get_events(&spec.task_id)
                .unwrap()
                .iter()
                .any(|event| event.detail == "hung_detected"
                    && event
                        .metadata
                        .as_ref()
                        .is_some_and(|value| value["transient"] == transient
                            && value["hung_duration_secs"] == 30))
        );
    }
}

#[tokio::test]
async fn stderr_activity_counts_as_progress_and_is_preserved() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, mut args) = setup();
    policy(&mut args, &[("AID_IDLE_TIMEOUT_SECS", "1")]);
    tokio::time::timeout(
        Duration::from_secs(5),
        run_process(
            &store,
            &spec,
            &args,
            &ClaudeAgent,
            command("stderr", &home.path().join("input")),
            std::time::Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status(&store), TaskStatus::Done);
    let transcript = std::fs::read_to_string(paths::transcript_path(&spec.task_id)).unwrap();
    assert!(transcript.contains("stderr progress"));
}

#[tokio::test]
async fn cost_ceiling_and_usage_checkpoints_survive_piped_routing() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, mut args) = setup();
    args.max_task_cost = Some(1.0);
    tokio::time::timeout(
        Duration::from_secs(120),
        run_process(
            &store,
            &spec,
            &args,
            &ClaudeAgent,
            gated_tests::gated_command("cost", &home.path().join("input")),
            std::time::Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status(&store), TaskStatus::Failed);
    let task = store.get_task(&spec.task_id).unwrap().unwrap();
    assert_eq!(task.cost_usd, Some(2.0));
    assert_eq!(task.tokens, Some(7));
    assert!(
        store
            .get_events(&spec.task_id)
            .unwrap()
            .iter()
            .any(|event| event.detail.contains("exceeded ceiling"))
    );
}

#[tokio::test]
async fn remote_wait_descendant_counts_as_activity_on_production_stream() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, mut args) = setup();
    policy(&mut args, &[("AID_IDLE_TIMEOUT_SECS", "1")]);
    tokio::time::timeout(
        Duration::from_secs(5),
        run_process(
            &store,
            &spec,
            &args,
            &ClaudeAgent,
            command("descendant", &home.path().join("input")),
            std::time::Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status(&store), TaskStatus::Done);
    assert!(
        !store
            .get_events(&spec.task_id)
            .unwrap()
            .iter()
            .any(|event| event.detail == "hung_detected")
    );
}

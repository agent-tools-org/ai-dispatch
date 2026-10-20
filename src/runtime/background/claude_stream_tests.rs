// Production branch regression tests: interactive specs must use Claude streaming stdin.
// Fake CLIs exercise replay receipts, refusal, early exit and blocked steer writes.
use super::*;
use crate::agent::claude::ClaudeAgent;
use crate::types::{MessageDirection, MessageSource, TaskStatus};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn setup() -> (
    tempfile::TempDir,
    paths::AidHomeGuard,
    Arc<Store>,
    BackgroundRunSpec,
    RunArgs,
) {
    let home = tempfile::tempdir().unwrap();
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES ('t-production',
         'claude','initial prompt','running',?1)",
            [chrono::Local::now().to_rfc3339()],
        )
        .unwrap();
    let spec: BackgroundRunSpec = serde_json::from_value(json!({
        "task_id":"t-production", "agent_name":"claude", "prompt":"initial prompt",
        "dir":null, "output":null, "verify":null, "retry":0, "group":null, "interactive":true,
    }))
    .unwrap();
    save_spec(&spec).unwrap();
    let args = RunArgs {
        output: Some(home.path().join("output.txt").display().to_string()),
        ..Default::default()
    };
    (home, guard, store, spec, args)
}

fn queue(store: &Store, content: &str) -> i64 {
    store
        .insert_message(
            "t-production",
            MessageDirection::In,
            content,
            MessageSource::Steer,
        )
        .unwrap()
        .id
}

fn command(mode: &str, output: &Path) -> std::process::Command {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut command = std::process::Command::new("python3");
    command
        .arg(fixtures.join("claude-production-input.py"))
        .arg(mode)
        .arg(output)
        .args([
            "-p",
            "initial prompt",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
    command
}

async fn wait_for_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        while !path.exists() {
            tick.tick().await;
        }
    })
    .await
    .unwrap();
}

fn status(store: &Store) -> TaskStatus {
    store.get_task("t-production").unwrap().unwrap().status
}

#[tokio::test]
async fn interactive_background_spec_accepts_only_replay_and_settles() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, args) = setup();
    let id = queue(&store, "guidance");
    let output = home.path().join("input.json");
    let observer = async {
        wait_for_file(&output).await;
        assert!(!crate::runtime::claude_input::receipt(&store, &spec.task_id, id).unwrap());
        assert_eq!(status(&store), TaskStatus::Running);
        std::fs::write(output.with_extension("replay"), "go").unwrap();
    };
    let run = run_process(&store, &spec, &args, &ClaudeAgent, command("ack", &output), std::time::Instant::now());
    let (result, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(run, observer)
    })
    .await
    .unwrap();
    result.unwrap();
    assert_eq!(status(&store), TaskStatus::Done);
    assert!(crate::runtime::claude_input::receipt(&store, &spec.task_id, id).unwrap());
    assert!(!crate::runtime::claude_input::active(&spec.task_id));
    assert!(output.with_extension("closed").exists());
    assert!(
        std::fs::read_to_string(args.output.as_ref().unwrap())
            .unwrap()
            .contains("final result")
    );
    assert!(
        std::fs::read_to_string(paths::transcript_path(&spec.task_id))
            .unwrap()
            .contains("final result")
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
async fn interactive_background_immediate_exit_preserves_status_and_output() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard, store, spec, args) = setup();
    for (code, expected) in [(0, TaskStatus::Done), (7, TaskStatus::Failed)] {
        store
            .db()
            .execute(
                "UPDATE tasks SET status='running' WHERE id='t-production'",
                [],
            )
            .unwrap();
        let mut command = std::process::Command::new("sh");
        command.args([
            "-c",
            &format!(
                concat!(
                    "printf '%s\\n' '{{\"type\":\"result\",\"subtype\":\"success\",",
                    "\"result\":\"startup output\"}}'; exit {}"
                ),
                code
            ),
            "claude",
            "-p",
            "initial prompt",
        ]);
        run_process(&store, &spec, &args, &ClaudeAgent, command, std::time::Instant::now())
            .await
            .unwrap();
        let task = store.get_task(&spec.task_id).unwrap().unwrap();
        assert_eq!(task.status, expected);
        assert_eq!(task.exit_code, Some(code));
        assert!(
            std::fs::read_to_string(paths::log_path(&spec.task_id))
                .unwrap()
                .contains("startup output")
        );
        assert!(!crate::runtime::claude_input::active(&spec.task_id));
    }
}

#[tokio::test]
async fn interactive_background_api_refusal_still_fails_without_reading_stdin() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard, store, mut spec, args) = setup();
    let route = crate::agent::api_route::ApiRouteAgent(
        serde_json::from_value(json!({
            "id":"synthetic-route", "agent":"claude", "keychain_service":"synthetic",
            "monthly_budget_usd":100,
        }))
        .unwrap(),
    );
    spec.agent_name = "synthetic-route".into();
    let mut command = std::process::Command::new("sh");
    command.args([
        "-c",
        concat!(
            "printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"apiKeySource\":\"none\"}'"
        ),
        "claude",
        "-p",
        "initial prompt",
    ]);
    run_process(&store, &spec, &args, &route, command, std::time::Instant::now())
        .await
        .unwrap();
    assert_eq!(status(&store), TaskStatus::Failed);
    assert!(
        store
            .get_events(&spec.task_id)
            .unwrap()
            .iter()
            .any(|event| event.detail == "route synthetic-route is not billed to its API key")
    );
    assert!(!crate::runtime::claude_input::active(&spec.task_id));
}

#[tokio::test]
async fn timed_out_steer_write_closes_input_without_failing_background_task() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, args) = setup();
    let id = queue(&store, &"x".repeat(2 * 1024 * 1024));
    let output = home.path().join("blocked.json");
    tokio::time::timeout(
        Duration::from_secs(12),
        run_process(
            &store,
            &spec,
            &args,
            &ClaudeAgent,
            command("blocked", &output),
            std::time::Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status(&store), TaskStatus::Done);
    assert!(output.with_extension("closed").exists());
    assert!(!crate::runtime::claude_input::receipt(&store, &spec.task_id, id).unwrap());
    assert!(
        store.list_messages_for_task(&spec.task_id).unwrap()[0]
            .delivered_at
            .is_none()
    );
    assert_eq!(
        store
            .get_events(&spec.task_id)
            .unwrap()
            .iter()
            .filter(|event| event
                .metadata
                .as_ref()
                .is_some_and(|value| value["claude_steer_message_id"] == id))
            .count(),
        1
    );
}

#[path = "claude_stream_policy_tests.rs"]
mod policy_tests;


#[path = "claude_stream_startup_tests.rs"]
mod startup_tests;
#[path = "claude_stream_completion_tests.rs"]
mod completion_tests;

// Event-driven fixture launch and observation for production stream regressions.
// Keeps fixture lifetime independent of watchdog and transport timeout budgets.
use super::*;

pub(super) fn gated_command(mode: &str, output: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("python3");
    command
        .args(["-c", include_str!("claude_stream_gated.py"), mode])
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

pub(super) async fn wait_for_write_failure(store: &Store, task: &str, id: i64) {
    tokio::time::timeout(Duration::from_secs(120), async {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            if !crate::runtime::claude_input::active(task)
                && store.get_events(task).unwrap().iter().any(|event| {
                    event
                        .metadata
                        .as_ref()
                        .is_some_and(|value| value["claude_steer_message_id"] == id)
                })
            {
                return;
            }
            tick.tick().await;
        }
    })
    .await
    .expect("steer write never settled");
}

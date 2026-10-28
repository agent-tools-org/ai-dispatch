// Webhook delivery for task completion notifications.
// Exports fire_webhooks() plus task-level dispatch helpers using curl.

use serde_json::json;
use std::process::{Command, Stdio};

use crate::config::{AidConfig, WebhookConfig};
use crate::store::Store;
use crate::types::{Task, TaskStatus};

pub async fn fire_task_webhooks(store: &Store, task_id: &str) {
    let task = match store.get_task(task_id) {
        Ok(Some(task)) => task,
        Ok(None) => return,
        Err(err) => return aid_error!("[aid] failed to load task {task_id} for webhooks: {err}"),
    };
    let Some(status) = webhook_status(task.status) else {
        return;
    };
    match crate::config::load_config() {
        Ok(config) => fire_webhooks(&config, &task, status).await,
        Err(err) => aid_error!("[aid] failed to load config for webhooks: {err}"),
    }
}

fn webhook_status(status: TaskStatus) -> Option<&'static str> {
    match status {
        TaskStatus::Done | TaskStatus::Merged => Some("done"),
        TaskStatus::Failed => Some("failed"),
        TaskStatus::Stopped => Some("stopped"),
        _ => return None,
    }
}

pub async fn fire_webhooks(config: &AidConfig, task: &Task, status: &str) {
    for webhook in &config.webhooks {
        if should_fire_webhook(webhook, status) {
            send_webhook(webhook, task, status).await;
        }
    }
}

fn should_fire_webhook(webhook: &WebhookConfig, status: &str) -> bool {
    (status == "done" && webhook.on_done)
        || ((status == "failed" || status == "stopped") && webhook.on_failed)
}

async fn send_webhook(webhook: &WebhookConfig, task: &Task, status: &str) {
    let mut cmd = Command::new("curl");
    cmd.arg("-fsS")
        .arg("-X")
        .arg("POST")
        .arg(&webhook.url)
        .arg("-H")
        .arg("Content-Type: application/json");
    for (key, value) in &webhook.headers {
        cmd.arg("-H").arg(format!("{key}: {value}"));
    }
    let body = webhook_payload(task, status);
    cmd.arg("-d")
        .arg(body.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    match cmd.spawn()
    {
        Ok(child) => {
            let name = webhook.name.clone();
            std::thread::spawn(move || match child.wait_with_output() {
                Ok(output) if !output.status.success() => aid_error!(
                    "[aid] webhook {name} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                Ok(_) => {}
                Err(err) => aid_error!("[aid] webhook {name} wait failed: {err}"),
            });
        }
        Err(err) => aid_error!("[aid] failed to fire webhook {}: {err}", webhook.name),
    }
}

fn webhook_payload(task: &Task, status: &str) -> serde_json::Value {
    json!({
        "task_id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "status": status,
        "outcome": task.outcome().as_str(),
        "verify_status": task.verify_status.as_str(),
        "prompt": task.prompt.as_str(),
        "duration_ms": task.duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::{should_fire_webhook, webhook_payload, webhook_status};
    use crate::config::WebhookConfig;
    use crate::types::{Task, TaskId, TaskStatus, VerifyStatus};


    #[test]
    fn webhook_payload_adds_outcome_and_verification_status() {
        let task = Task {
            verify: Some("cargo test".to_string()),
            verify_status: VerifyStatus::TimedOut,
            ..crate::types::Task::test_fixture(TaskId("t-webhook".to_string()), TaskStatus::Done)
        };

        let payload = webhook_payload(&task, "done");

        assert_eq!(payload["status"], "done");
        assert_eq!(payload["outcome"], "unverified");
        assert_eq!(payload["verify_status"], "timed_out");
    }

    #[test]
    fn stopped_tasks_emit_distinct_terminal_webhook_status() {
        assert_eq!(webhook_status(TaskStatus::Stopped), Some("stopped"));
        assert_eq!(webhook_status(TaskStatus::Failed), Some("failed"));
    }

    #[test]
    fn stopped_tasks_use_terminal_webhook_subscription() {
        let webhook = WebhookConfig {
            name: "terminal".to_string(),
            url: "https://example.test/hook".to_string(),
            on_done: false,
            on_failed: true,
            headers: Vec::new(),
        };

        assert!(should_fire_webhook(&webhook, "stopped"));
        assert!(!should_fire_webhook(&webhook, "done"));
    }
}

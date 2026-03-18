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
    let status = match task.status {
        TaskStatus::Done | TaskStatus::Merged => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Stopped => "failed",
        _ => return,
    };
    match crate::config::load_config() {
        Ok(config) => fire_webhooks(&config, &task, status).await,
        Err(err) => aid_error!("[aid] failed to load config for webhooks: {err}"),
    }
}

pub async fn fire_webhooks(config: &AidConfig, task: &Task, status: &str) {
    for webhook in &config.webhooks {
        if (status == "done" && webhook.on_done) || (status == "failed" && webhook.on_failed) {
            send_webhook(webhook, task, status).await;
        }
    }
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
    let body = json!({
        "task_id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "status": status,
        "prompt": task.prompt.as_str(),
        "duration_ms": task.duration_ms,
    });
    match cmd
        .arg("-d")
        .arg(body.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
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

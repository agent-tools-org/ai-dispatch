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
    use super::webhook_payload;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    #[test]
    fn webhook_payload_adds_outcome_and_verification_status() {
        let task = Task {
            id: TaskId("t-webhook".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            final_head_sha: None,
            final_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            requested_model: None,
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: Some("cargo test".to_string()),
            verify_status: VerifyStatus::TimedOut,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        };

        let payload = webhook_payload(&task, "done");

        assert_eq!(payload["status"], "done");
        assert_eq!(payload["outcome"], "unverified");
        assert_eq!(payload["verify_status"], "timed_out");
    }
}

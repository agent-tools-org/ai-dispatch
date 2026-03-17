// Watcher engine: reads agent stdout/stderr and records events to store.
// Supports streaming JSONL (codex/opencode) and buffered JSON (gemini) modes.

use anyhow::Result;
use chrono::Local;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::time::{timeout, Duration};

const HUNG_TIMEOUT: Duration = Duration::from_secs(300);

use crate::agent::Agent;
use crate::paths;
use crate::rate_limit;
use crate::store::Store;
use crate::types::*;

/// Watch a child process, parse output, store events, return completion info
pub async fn watch_streaming(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    workgroup_id: Option<&str>,
    max_task_cost: Option<f64>,
) -> Result<CompletionInfo> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Open log file for raw output
    let mut log_file = tokio::fs::File::create(log_path).await?;
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let mut event_count = 0u32;
    let mut session_saved = false;
    let mut loop_detector = LoopDetector::new();

    // Spawn stderr capture in background
    let mut stderr_handle = spawn_stderr_capture(child, task_id);

    loop {
        let line = match timeout(HUNG_TIMEOUT, lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {
                let _ = child.kill().await;
                let event = TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: format!(
                        "Agent hung: no output for {} seconds",
                        HUNG_TIMEOUT.as_secs()
                    ),
                    metadata: None,
                };
                let _ = store.insert_event(&event);
                info.status = TaskStatus::Failed;
                break;
            }
        };

        use tokio::io::AsyncWriteExt;
        if extract_milestone_detail(&line).is_none() {
            log_file.write_all(line.as_bytes()).await?;
            log_file.write_all(b"\n").await?;
        }

        if let Some(detail) = handle_streaming_line_with_session(
            StreamLineContext {
                agent,
                task_id,
                store,
                workgroup_id,
            },
            &mut info,
            &mut event_count,
            &line,
            &mut session_saved,
        )? {
            if exceeds_cost_ceiling(info.cost_usd, max_task_cost) {
                let current_cost = info.cost_usd.unwrap_or_default();
                let max_cost = max_task_cost.unwrap_or_default();
                let _ = store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: format!(
                        "Task killed: cost ${:.2} exceeded ceiling ${:.2}",
                        current_cost, max_cost
                    ),
                    metadata: None,
                });
                let _ = child.kill().await;
                info.status = TaskStatus::Failed;
                break;
            }
            loop_detector.push(&detail);
            if loop_detector.is_looping() {
                let _ = store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: "Agent appears stuck in a loop — killing process".to_string(),
                    metadata: None,
                });
                let _ = child.kill().await;
                info.status = TaskStatus::Failed;
                if let Some(handle) = stderr_handle.take() { let _ = handle.await; }
                return Ok(info);
            }
        }
    }

    // Wait for stderr to finish
    if let Some(handle) = stderr_handle.take() {
        let _ = handle.await;
    }

    // Wait for process exit
    let exit_status = child.wait().await?;
    let status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };

    // Auto-clear rate limit on successful completion
    if status == TaskStatus::Done && rate_limit::is_rate_limited(&agent.kind()) {
        rate_limit::clear_rate_limit(&agent.kind());
    }

    // Record final event (include stderr hint on failure)
    let stderr_note = if status == TaskStatus::Failed {
        let stderr_path = paths::stderr_path(task_id.as_str());
        if stderr_path.exists() {
            if let Ok(stderr_content) = std::fs::read_to_string(&stderr_path) {
                for line in stderr_content.lines() {
                    if rate_limit::is_rate_limit_error(line) {
                        rate_limit::mark_rate_limited(&agent.kind(), line);
                        break;
                    }
                }
            }
            format!(" — stderr: {}", stderr_path.display())
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let detail = format!(
        "{} — {} events, exit code {}{}",
        status.label(),
        event_count,
        exit_status.code().unwrap_or(-1),
        stderr_note,
    );
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: if status == TaskStatus::Done {
            EventKind::Completion
        } else {
            EventKind::Error
        },
        detail,
        metadata: None,
    })?;

    info.status = status;
    Ok(info)
}

/// Watch a non-streaming agent: buffer all output, parse at end
pub async fn watch_buffered(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    output_path: Option<&std::path::Path>,
    _workgroup_id: Option<&str>,
) -> Result<CompletionInfo> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let mut reader = BufReader::new(stdout);
    let mut buffer = String::new();

    // Spawn stderr capture in background
    let stderr_handle = spawn_stderr_capture(child, task_id);

    // Read all output
    use tokio::io::AsyncReadExt;
    reader.read_to_string(&mut buffer).await?;

    // Write raw output to log (filter out milestone lines)
    let filtered: String = buffer
        .lines()
        .filter(|line| extract_milestone_detail(line).is_none())
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(log_path, &filtered).await?;

    // Write response to output file if requested
    if let Some(out_path) = output_path {
        if let Some(response) = crate::agent::gemini::extract_response(&buffer) {
            let response_filtered: String = response
                .lines()
                .filter(|line| extract_milestone_detail(line).is_none())
                .collect::<Vec<_>>()
                .join("\n");
            tokio::fs::write(out_path, &response_filtered).await?;
        } else {
            tokio::fs::write(out_path, &filtered).await?;
        }
    }

    // Wait for stderr to finish
    if let Some(handle) = stderr_handle {
        let _ = handle.await;
    }

    // Wait for process exit
    let exit_status = child.wait().await?;

    let mut info = if exit_status.success() {
        agent.parse_completion(&buffer)
    } else {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Failed,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    };
    info.exit_code = exit_status.code();

    // Record completion event
    let event = crate::agent::gemini::make_completion_event(task_id, &info);
    store.insert_event(&event)?;

    Ok(info)
}

/// Spawn a background task to capture stderr to a file
fn spawn_stderr_capture(
    child: &mut Child,
    task_id: &TaskId,
) -> Option<tokio::task::JoinHandle<()>> {
    let stderr = child.stderr.take()?;
    let stderr_path = paths::stderr_path(task_id.as_str());
    Some(tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut collected = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push_str(&line);
            collected.push('\n');
        }
        if !collected.is_empty() {
            let _ = tokio::fs::write(&stderr_path, &collected).await;
        }
    }))
}

fn apply_completion_event(info: &mut CompletionInfo, event: &TaskEvent) {
    if event.event_kind != EventKind::Completion {
        return;
    }
    let Some(metadata) = event.metadata.as_ref() else {
        return;
    };

    if let Some(tokens) = metadata.get("tokens").and_then(|value| value.as_i64()) {
        info.tokens = Some(tokens);
    }
    if let Some(model) = metadata.get("model").and_then(|value| value.as_str()) {
        info.model = Some(model.to_string());
    }
    if let Some(cost_usd) = metadata.get("cost_usd").and_then(|value| value.as_f64()) {
        info.cost_usd = Some(cost_usd);
    }
}

fn exceeds_cost_ceiling(current_cost: Option<f64>, max_task_cost: Option<f64>) -> bool {
    matches!(
        (current_cost, max_task_cost),
        (Some(current_cost), Some(max_task_cost)) if current_cost > max_task_cost
    )
}

pub(crate) struct StreamLineContext<'a> {
    pub agent: &'a dyn Agent,
    pub task_id: &'a TaskId,
    pub store: &'a Arc<Store>,
    pub workgroup_id: Option<&'a str>,
}

pub(crate) fn handle_streaming_line(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    info: &mut CompletionInfo,
    event_count: &mut u32,
    workgroup_id: Option<&str>,
    line: &str,
) -> Result<()> {
    if let Some(finding) = extract_finding_detail(line)
        && let Some(group_id) = workgroup_id
    {
        let _ = store.insert_finding(group_id, &finding, Some(task_id.as_str()));
        append_to_broadcast(group_id, task_id.as_str(), &finding);
    }
    if let Some(event) = parse_milestone_event(task_id, line) {
        store.insert_event(&event)?;
        *event_count += 1;
        return Ok(());
    }
    if let Some(event) = agent.parse_event(task_id, line) {
        apply_completion_event(info, &event);
        store.insert_event(&event)?;
        *event_count += 1;
    }
    Ok(())
}

pub(crate) fn handle_streaming_line_with_session(
    ctx: StreamLineContext<'_>,
    info: &mut CompletionInfo,
    event_count: &mut u32,
    line: &str,
    session_saved: &mut bool,
) -> Result<Option<String>> {
    let StreamLineContext {
        agent,
        task_id,
        store,
        workgroup_id,
    } = ctx;
    if let Some(finding) = extract_finding_detail(line)
        && let Some(group_id) = workgroup_id
    {
        let _ = store.insert_finding(group_id, &finding, Some(task_id.as_str()));
        append_to_broadcast(group_id, task_id.as_str(), &finding);
    }
    if let Some(event) = parse_milestone_event(task_id, line) {
        store.insert_event(&event)?;
        *event_count += 1;
        return Ok(Some(event.detail.clone()));
    }
    if let Some(event) = agent.parse_event(task_id, line) {
        apply_completion_event(info, &event);
        if !*session_saved
            && let Some(metadata) = &event.metadata
            && let Some(session_id) = metadata.get("agent_session_id").and_then(|s| s.as_str())
        {
            store.update_agent_session_id(task_id.as_str(), session_id)?;
            *session_saved = true;
        }
        // Detect rate limit errors in streaming output (cursor sends these via stdout JSON)
        if rate_limit::is_rate_limit_error(&event.detail) {
            rate_limit::mark_rate_limited(&agent.kind(), &event.detail);
        }
        store.insert_event(&event)?;
        *event_count += 1;
        return Ok(Some(event.detail.clone()));
    }
    Ok(None)
}

fn parse_milestone_event(task_id: &TaskId, line: &str) -> Option<TaskEvent> {
    let detail = extract_milestone_detail(line)?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: None,
    })
}

fn extract_milestone_detail(line: &str) -> Option<String> {
    if !line.contains("[MILESTONE]") {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
        && let Some(detail) = extract_milestone_from_json(&value)
    {
        return Some(detail);
    }
    // If line looks like JSON but failed parsing, tag is inside a string value — skip
    let trimmed = line.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return None;
    }
    extract_milestone_from_text(line)
}

fn extract_milestone_from_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => extract_milestone_from_text(text),
        serde_json::Value::Array(items) => items.iter().find_map(extract_milestone_from_json),
        serde_json::Value::Object(map) => map.values().find_map(extract_milestone_from_json),
        _ => None,
    }
}

const MILESTONE_TAG: &str = "[MILESTONE]";
const FINDING_TAG: &str = "[FINDING]";

fn tag_is_inside_code_string(line: &str, tag_pos: usize) -> bool {
    let before = &line[..tag_pos];
    let single_quotes = before.chars().filter(|&c| c == '\'').count();
    let double_quotes = before.chars().filter(|&c| c == '"').count();
    if single_quotes % 2 == 1 || double_quotes % 2 == 1 {
        return true;
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("///") {
        return true;
    }
    if trimmed.starts_with("println!")
        || trimmed.starts_with("eprintln!")
        || trimmed.starts_with("console.log")
    {
        return true;
    }
    false
}

fn extract_milestone_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let tag_pos = line.find(MILESTONE_TAG)?;
        if tag_is_inside_code_string(line, tag_pos) {
            return None;
        }
        let detail = line[tag_pos + MILESTONE_TAG.len()..]
            .trim()
            .trim_start_matches(':')
            .trim();
        if detail.is_empty() {
            None
        } else {
            Some(detail.to_string())
        }
    })
}

fn extract_finding_detail(line: &str) -> Option<String> {
    if !line.contains("[FINDING]") {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
        && let Some(detail) = extract_finding_from_json(&value)
    {
        return Some(detail);
    }
    let trimmed = line.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return None;
    }
    extract_finding_from_text(line)
}

fn append_to_broadcast(workgroup_id: &str, task_id: &str, content: &str) {
    let broadcast_path = crate::paths::workspace_dir(workgroup_id).join("broadcast.md");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&broadcast_path)
    {
        use std::io::Write;
        let timestamp = Local::now().format("%H:%M:%S");
        let _ = writeln!(file, "- [{timestamp}] ({task_id}) {content}");
    }
}

fn extract_finding_from_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => extract_finding_from_text(text),
        serde_json::Value::Array(items) => items.iter().find_map(extract_finding_from_json),
        serde_json::Value::Object(map) => map.values().find_map(extract_finding_from_json),
        _ => None,
    }
}

fn extract_finding_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let tag_pos = line.find(FINDING_TAG)?;
        if tag_is_inside_code_string(line, tag_pos) {
            return None;
        }
        let detail = line[tag_pos + FINDING_TAG.len()..]
            .trim()
            .trim_start_matches(':')
            .trim();
        if detail.is_empty() {
            None
        } else {
            Some(detail.to_string())
        }
    })
}

struct LoopDetector {
    recent_events: VecDeque<String>,
}

impl LoopDetector {
    fn new() -> Self { Self { recent_events: VecDeque::new() } }
    fn push(&mut self, detail: &str) {
        // Skip empty/whitespace-only details to avoid false loop detection
        if detail.trim().is_empty() {
            return;
        }
        let mut truncated = detail.to_string();
        truncated.truncate(100);
        self.recent_events.push_back(truncated);
        if self.recent_events.len() > 20 { self.recent_events.pop_front(); }
    }
    fn is_looping(&self) -> bool {
        if self.recent_events.len() < 10 { return false; }
        let mut counts = HashMap::new();
        for detail in self.recent_events.iter().rev().take(10) {
            let counter = counts.entry(detail.as_str()).or_insert(0);
            *counter += 1;
            if *counter >= 8 { return true; }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_completion_event, exceeds_cost_ceiling, parse_milestone_event, LoopDetector};
    use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
    use chrono::Local;
    use serde_json::json;

    #[test]
    fn completion_metadata_updates_summary_fields() {
        let mut info = CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        };
        let event = TaskEvent {
            task_id: TaskId("t-usage".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Completion,
            detail: "completed".to_string(),
            metadata: Some(json!({
                "tokens": 12345,
                "model": "gpt-4.1",
                "cost_usd": 0.12
            })),
        };

        apply_completion_event(&mut info, &event);

        assert_eq!(info.tokens, Some(12345));
        assert_eq!(info.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(info.cost_usd, Some(0.12));
    }

    #[test]
    fn non_completion_events_do_not_change_summary_fields() {
        let mut info = CompletionInfo {
            tokens: Some(10),
            status: TaskStatus::Done,
            model: Some("gpt-4.1".to_string()),
            cost_usd: Some(0.01),
            exit_code: None,
        };
        let event = TaskEvent {
            task_id: TaskId("t-ignore".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "thinking".to_string(),
            metadata: Some(json!({ "tokens": 999 })),
        };

        apply_completion_event(&mut info, &event);

        assert_eq!(info.tokens, Some(10));
        assert_eq!(info.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(info.cost_usd, Some(0.01));
    }

    #[test]
    fn cost_ceiling_only_triggers_above_limit() {
        assert!(!exceeds_cost_ceiling(Some(1.0), Some(1.0)));
        assert!(exceeds_cost_ceiling(Some(1.01), Some(1.0)));
        assert!(!exceeds_cost_ceiling(None, Some(1.0)));
        assert!(!exceeds_cost_ceiling(Some(1.0), None));
    }

    #[test]
    fn milestone_event_parses_plain_text_lines() {
        let event = parse_milestone_event(
            &TaskId("t-m1".to_string()),
            "[MILESTONE] types defined",
        )
        .unwrap();

        assert_eq!(event.event_kind, EventKind::Milestone);
        assert_eq!(event.detail, "types defined");
    }

    #[test]
    fn milestone_event_parses_json_lines() {
        let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"[MILESTONE] tests passing\nnext"}} "#;
        let event = parse_milestone_event(&TaskId("t-m2".to_string()), line).unwrap();

        assert_eq!(event.event_kind, EventKind::Milestone);
        assert_eq!(event.detail, "tests passing");
    }

    #[test]
    fn finding_event_parses_plain_text_lines() {
        let detail = super::extract_finding_detail("[FINDING] gamma can be zero in tricrypto");
        assert_eq!(detail.as_deref(), Some("gamma can be zero in tricrypto"));
    }

    #[test]
    fn milestone_inside_string_literal_is_rejected() {
        let line = r#"println!("[MILESTONE] tests passing");"#;
        assert!(super::extract_milestone_detail(line).is_none());
    }

    #[test]
    fn milestone_inside_json_string_value_is_rejected() {
        let line = r#"{"text": "assert_eq!(detail, "[MILESTONE] done")"}"#;
        assert!(super::extract_milestone_detail(line).is_none());
    }

    #[test]
    fn finding_inside_string_literal_is_rejected() {
        let line = r#"let s = "[FINDING] gamma can be zero";"#;
        assert!(super::extract_finding_detail(line).is_none());
    }

    #[test]
    fn real_milestone_still_extracted() {
        let detail = super::extract_milestone_detail("[MILESTONE] implementation complete");
        assert_eq!(detail.as_deref(), Some("implementation complete"));
    }

    #[test]
    fn real_finding_still_extracted() {
        let detail = super::extract_finding_detail("[FINDING] pool has degenerate state");
        assert_eq!(detail.as_deref(), Some("pool has degenerate state"));
    }

    #[test]
    fn milestone_lines_stripped_from_output() {
        let input = "line1\n[MILESTONE] types defined\nline2\n";
        let filtered: String = input
            .lines()
            .filter(|line| super::extract_milestone_detail(line).is_none())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(filtered, "line1\nline2");
    }

    fn loop_detector_case<I>(expected: bool, events: I) where I: IntoIterator<Item = &'static str> {
        let mut detector = LoopDetector::new();
        events.into_iter().for_each(|detail| detector.push(detail));
        assert_eq!(detector.is_looping(), expected);
    }
    #[test]
    fn loop_detector_patterns() {
        loop_detector_case(false, ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        loop_detector_case(true, std::iter::repeat("repeat").take(10));
        loop_detector_case(
            false,
            std::iter::repeat("dup").take(7).chain(["unique-1", "unique-2", "unique-3"]),
        );
        loop_detector_case(
            true,
            std::iter::repeat("dup").take(8).chain(["unique-1", "unique-2"]),
        );
    }

    #[test]
    fn loop_detector_ignores_empty_details() {
        // Empty/whitespace details should not trigger loop detection
        loop_detector_case(false, std::iter::repeat("").take(20));
        loop_detector_case(false, std::iter::repeat("  ").take(20));
        loop_detector_case(false, std::iter::repeat("\t").take(20));
        // Mix of empty and real events should not false-positive
        let mut events: Vec<&str> = Vec::new();
        for _ in 0..5 {
            events.push("");
            events.push("working");
            events.push("  ");
        }
        loop_detector_case(false, events);
    }
}

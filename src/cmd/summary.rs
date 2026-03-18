// Generate structured completion summaries for finished tasks.
// Exports: generate_summary(), CompletionSummary.
// Deps: crate::types::Task, crate::cmd::judge, summary_conclusion.
use crate::cmd::judge;
use crate::types::Task;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[path = "summary_conclusion.rs"]
mod summary_conclusion;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionSummary {
    pub task_id: String,
    pub agent: String,
    pub status: String,
    pub files_changed: Vec<String>,
    pub summary_text: String,
    #[serde(default)]
    pub conclusion: String,
    pub duration_secs: Option<i64>,
    pub token_count: Option<i64>,
}

pub fn generate_summary(task: &Task) -> CompletionSummary {
    let files_changed = judge::gather_diff(task)
        .map(|diff| extract_files_from_diff(&diff))
        .unwrap_or_default();
    let file_list = format_file_list(&files_changed);
    let duration_secs = task.duration_ms.map(|ms| ms / 1_000);
    let duration_label = format_duration(duration_secs);
    let conclusion = summary_conclusion::extract_conclusion(task);
    let summary_text = format!(
        "{} {}: {} files changed ({}). Duration: {}.",
        task.agent_display_name(),
        task.status.as_str(),
        files_changed.len(),
        file_list,
        duration_label
    );

    CompletionSummary {
        task_id: task.id.as_str().to_string(),
        agent: task.agent_display_name().to_string(),
        status: task.status.as_str().to_string(),
        files_changed,
        summary_text,
        conclusion,
        duration_secs,
        token_count: task.tokens,
    }
}

pub fn format_summary_for_injection(summary: &CompletionSummary) -> String {
    let duration = format_duration(summary.duration_secs);
    let files = format_file_list(&summary.files_changed);
    format!(
        "## Parent Task Context ({})\nAgent: {} | Status: {} | Duration: {}\nFiles changed: {}\nConclusion: {}",
        summary.task_id,
        summary.agent,
        summary.status,
        duration,
        files,
        display_conclusion(&summary.conclusion)
    )
}

pub fn format_sibling_summaries(summaries: &[CompletionSummary]) -> String {
    if summaries.is_empty() { return String::new(); }
    let mut lines = vec!["## Sibling Task Context".to_string()];
    for s in summaries {
        lines.push(format!(
            "- {} ({}): {} | Files: {} | Conclusion: {}",
            s.task_id,
            s.agent,
            s.status,
            if s.files_changed.is_empty() { "(none)".to_string() }
            else { s.files_changed.join(", ") },
            display_conclusion(&s.conclusion)
        ));
    }
    lines.join("\n")
}

fn format_file_list(files: &[String]) -> String {
    if files.is_empty() {
        "no changes detected".to_string()
    } else {
        files.join(", ")
    }
}

fn format_duration(duration_secs: Option<i64>) -> String {
    match duration_secs {
        Some(secs) => format!("{}s", secs),
        None => "unknown".to_string(),
    }
}

fn display_conclusion(conclusion: &str) -> String {
    if conclusion.is_empty() {
        "(none)".to_string()
    } else {
        conclusion.to_string()
    }
}

fn extract_files_from_diff(diff: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ")
            && let Some((a_path, _)) = rest.split_once(' ')
        {
            let normalized = a_path.strip_prefix("a/").unwrap_or(a_path);
            if seen.insert(normalized.to_string()) {
                files.push(normalized.to_string());
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;
    use tempfile::NamedTempFile;

    fn build_task() -> Task {
        Task {
            id: TaskId("t-summary".into()), agent: AgentKind::Codex, custom_agent_name: None,
            prompt: "test".into(), resolved_prompt: None,
            status: TaskStatus::Done, parent_task_id: None, workgroup_id: None,
            caller_kind: None, caller_session_id: None, agent_session_id: None,
            repo_path: None, worktree_path: None, worktree_branch: None,
            log_path: None, output_path: None,
            tokens: Some(42),
            prompt_tokens: Some(5),
            duration_ms: Some(2_500),
            model: None,
            cost_usd: None,
            exit_code: Some(0),
            created_at: Local::now(), completed_at: None,
            verify: None, verify_status: VerifyStatus::Pending,
            read_only: false,
            budget: false,
        }
    }
    #[test]
    fn generates_summary_from_task() {
        let task = build_task();
        let summary = generate_summary(&task);
        assert_eq!(summary.task_id, "t-summary");
        assert_eq!(summary.agent, "codex");
        assert_eq!(summary.status, "done");
        assert_eq!(summary.duration_secs, Some(2));
        assert!(summary.conclusion.is_empty());
        assert!(summary.summary_text.contains("codex done")); assert!(summary.summary_text.contains("(no changes detected)"));
    }
    #[test]
    fn format_summary_produces_readable_output() {
        let summary = CompletionSummary {
            task_id: "t0".into(),
            agent: "agent".into(),
            status: "done".into(),
            files_changed: vec!["src/lib.rs".into()],
            summary_text: String::new(),
            conclusion: "Implemented the retry logic.".into(),
            duration_secs: Some(3),
            token_count: None,
        };
        let formatted = format_summary_for_injection(&summary);
        assert_eq!(formatted, "## Parent Task Context (t0)\nAgent: agent | Status: done | Duration: 3s\nFiles changed: src/lib.rs\nConclusion: Implemented the retry logic.");
    }
    #[test]
    fn handles_no_diff_gracefully() {
        let summary = generate_summary(&build_task());
        assert!(summary.summary_text.contains("no changes detected")); assert!(summary.files_changed.is_empty());
    }
    #[test]
    fn extracts_files_from_diff() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\ndiff --git a/tests/helpers.rs b/tests/helpers.rs";
        let files = extract_files_from_diff(diff);
        assert_eq!(files, vec!["src/main.rs", "tests/helpers.rs"]);
    }
    #[test]
    fn format_sibling_summaries_renders_list() {
        let summaries = vec![
            CompletionSummary { task_id: "t-1".into(), agent: "codex".into(), status: "done".into(), files_changed: vec!["src/a.rs".into()], summary_text: "...".into(), conclusion: "Implemented retry logic.".into(), duration_secs: Some(60), token_count: None },
            CompletionSummary { task_id: "t-2".into(), agent: "gemini".into(), status: "done".into(), files_changed: vec![], summary_text: "...".into(), conclusion: String::new(), duration_secs: None, token_count: None },
        ];
        let output = format_sibling_summaries(&summaries);
        assert!(output.contains("Sibling Task Context"));
        assert!(output.contains("t-1"));
        assert!(output.contains("src/a.rs"));
        assert!(output.contains("Implemented retry logic."));
        assert!(output.contains("Conclusion: (none)"));
        assert!(output.contains("(none)"));
    }

    #[test]
    fn format_sibling_summaries_empty_returns_empty() {
        assert!(format_sibling_summaries(&[]).is_empty());
    }

    #[test]
    fn generates_conclusion_from_output_file() {
        let output = NamedTempFile::new().unwrap();
        std::fs::write(
            output.path(),
            "progress update\n\nImplemented the retry logic with exponential backoff and failure classification.",
        )
        .unwrap();
        let mut task = build_task();
        task.output_path = Some(output.path().display().to_string());
        let summary = generate_summary(&task);
        assert_eq!(
            summary.conclusion,
            "Implemented the retry logic with exponential backoff and failure classification."
        );
    }

    #[test]
    fn generates_conclusion_from_log_file_when_output_missing() {
        let log = NamedTempFile::new().unwrap();
        std::fs::write(
            log.path(),
            concat!(
                "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"planning\",\"delta\":false}\n",
                "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Implemented the retry logic with exponential backoff.\",\"delta\":false}\n"
            ),
        )
        .unwrap();
        let mut task = build_task();
        task.log_path = Some(log.path().display().to_string());
        let summary = generate_summary(&task);
        assert_eq!(
            summary.conclusion,
            "Implemented the retry logic with exponential backoff."
        );
    }

    #[test]
    fn deserializes_missing_conclusion_as_empty() {
        let summary: CompletionSummary = serde_json::from_str(
            "{\"task_id\":\"t-1\",\"agent\":\"codex\",\"status\":\"done\",\"files_changed\":[],\"summary_text\":\"...\",\"duration_secs\":1,\"token_count\":2}",
        )
        .unwrap();
        assert!(summary.conclusion.is_empty());
    }
}

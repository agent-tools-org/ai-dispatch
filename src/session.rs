// Caller session detection for aid dispatches, board filtering, and advise pools.
// Exports current_caller(), caller_agent(), caller_model() plus ownership display.

use crate::types::{AgentKind, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerSession {
    pub kind: String,
    pub session_id: String,
}

pub fn current_caller() -> Option<CallerSession> {
    explicit_caller().or_else(detect_known_caller)
}

pub fn matches_current(task: &Task) -> bool {
    let Some(current) = current_caller() else {
        return false;
    };
    task.caller_kind.as_deref() == Some(current.kind.as_str())
        && task.caller_session_id.as_deref() == Some(current.session_id.as_str())
}

pub fn display(task: &Task) -> String {
    match (
        task.caller_kind.as_deref(),
        task.caller_session_id.as_deref(),
    ) {
        (Some(kind), Some(session_id)) => {
            format!("{kind}:{}", shorten(session_id))
        }
        (Some(kind), None) => kind.to_string(),
        _ => "-".to_string(),
    }
}

/// The built-in agent whose provider pool a caller session draws on.
pub fn caller_agent(kind: &str) -> Option<AgentKind> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "claude-code" | "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        _ => None,
    }
}

/// The caller's own model: `--caller-model` when given, else `AID_CALLER_MODEL`.
pub fn caller_model(flag: Option<&str>) -> Option<String> {
    caller_model_from(flag, std::env::var("AID_CALLER_MODEL").ok().as_deref())
}

fn caller_model_from(flag: Option<&str>, env: Option<&str>) -> Option<String> {
    [flag, env]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn explicit_caller() -> Option<CallerSession> {
    let kind = std::env::var("AID_CALLER_KIND").ok()?;
    let session_id = std::env::var("AID_CALLER_SESSION").ok()?;
    Some(CallerSession { kind, session_id })
}

fn detect_known_caller() -> Option<CallerSession> {
    env_session("CODEX_THREAD_ID", "codex")
        .or_else(|| env_session("CLAUDECODE_SESSION_ID", "claude-code"))
        .or_else(|| env_session("CLAUDE_CODE_SESSION_ID", "claude-code"))
        .or_else(|| env_session("CURSOR_SESSION_ID", "cursor"))
        .or_else(fallback_terminal_session)
}

fn env_session(key: &str, kind: &str) -> Option<CallerSession> {
    let session_id = std::env::var(key).ok()?;
    Some(CallerSession {
        kind: kind.to_string(),
        session_id,
    })
}

fn fallback_terminal_session() -> Option<CallerSession> {
    let session_id = std::env::var("SECURITYSESSIONID")
        .or_else(|_| std::env::var("TERM_SESSION_ID"))
        .ok()?;
    let kind = std::env::var("TERM_PROGRAM").unwrap_or_else(|_| "terminal".to_string());
    Some(CallerSession { kind, session_id })
}

fn shorten(session_id: &str) -> &str {
    const MAX_LEN: usize = 8;
    if session_id.len() <= MAX_LEN {
        session_id
    } else {
        &session_id[..MAX_LEN]
    }
}

#[cfg(test)]
mod tests {
    use super::{caller_agent, caller_model_from, display};

    #[test]
    fn caller_sessions_map_to_their_pool_agent() {
        assert_eq!(caller_agent("claude-code"), Some(AgentKind::Claude));
        assert_eq!(caller_agent("codex"), Some(AgentKind::Codex));
        assert_eq!(caller_agent("iTerm.app"), None);
    }

    #[test]
    fn caller_model_flag_wins_over_env_and_blank_is_absent() {
        assert_eq!(caller_model_from(Some("opus"), Some("sonnet")).as_deref(), Some("opus"));
        assert_eq!(caller_model_from(None, Some("sonnet")).as_deref(), Some("sonnet"));
        assert_eq!(caller_model_from(Some(" "), None), None);
    }
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    #[test]
    fn display_shortens_long_session_ids() {
        let task = Task {
            id: TaskId("t-1234".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Running,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: Some("codex".to_string()),
            caller_session_id: Some("0123456789abcdef".to_string()),
            agent_session_id: None,
            repo_path: None, project_id: None,
            worktree_path: None, effective_dir: None,
            worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            requested_model: None, observed_model: None, attribution_source: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        };

        assert_eq!(display(&task), "codex:01234567");
    }
}

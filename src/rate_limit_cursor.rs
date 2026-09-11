// Cursor monthly refusal anchoring and cycle-end recovery.
// Exports (to signatures): NEEDLE, matches, cycle_end; deps: chrono.

use chrono::{Local, NaiveDate, NaiveDateTime};

pub(super) const NEEDLE: &str = "actionrequirederror: you've hit your usage limit";

pub(super) fn matches(lower: &str, needle: &str) -> bool {
    if needle == NEEDLE {
        return lower.lines().any(|line| line.trim_start().starts_with(NEEDLE));
    }
    lower.contains(needle)
}

pub(super) fn cycle_end(lower: &str) -> Option<NaiveDateTime> {
    let line = lower.lines().find(|line| line.trim_start().starts_with(NEEDLE))?;
    let rest = line.split_once("your monthly cycle ends on ")?.1;
    let date = rest.split_whitespace().next()?.trim_end_matches('.');
    let year = date.rsplit_once('/')?.1;
    if year.len() != 4 || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let date = NaiveDate::parse_from_str(date, "%m/%d/%Y").ok()?;
    // Hold through the entire UTC date; marker precision is one minute.
    let end = date.succ_opt()?.and_hms_opt(0, 0, 0)?.and_utc();
    Some(end.with_timezone(&Local).naive_local())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream_completion::{record_quota_exhaustion, QuotaOutcome};
    use crate::rate_limit;
    use crate::types::{AgentKind, TaskId};

    const CAPTURE: &str = include_str!("../tests/fixtures/cursor-usage-limit-refusal.txt");

    fn running_cursor_task() -> crate::types::Task {
        crate::types::Task {
            id: TaskId("t-cursor-monthly-replay".to_string()), agent: AgentKind::Cursor,
            custom_agent_name: None, prompt: "captured refusal replay".to_string(),
            resolved_prompt: None, category: None, status: crate::types::TaskStatus::Running,
            parent_task_id: None, workgroup_id: None, caller_kind: None, caller_session_id: None,
            agent_session_id: None, repo_path: None, project_id: None, worktree_path: None,
            effective_dir: None, worktree_branch: None, final_head_sha: None, final_branch: None,
            start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
            duration_ms: None, requested_model: Some("Auto".to_string()), observed_model: None,
            attribution_source: None, cost_usd: None, exit_code: None, created_at: Local::now(),
            completed_at: None, verify: None, verify_status: crate::types::VerifyStatus::Skipped,
            pending_reason: None, read_only: false, budget: false, audit_verdict: None,
            audit_report_path: None, delivery_assessment: None,
        }
    }

    #[tokio::test]
    async fn captured_cursor_stderr_replay_through_watcher_writes_auto_hold() {
        let temp = tempfile::tempdir().expect("temp home");
        let _home = crate::paths::AidHomeGuard::set(temp.path());
        let _cache = crate::live_quota::CacheDirGuard::set(temp.path());
        crate::paths::ensure_dirs().expect("aid directories");
        let store = std::sync::Arc::new(crate::store::Store::open_memory().expect("store"));
        let task = running_cursor_task();
        store.insert_task(&task).expect("task");
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cursor-usage-limit-refusal.txt");
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "cat \"$1\" >&2; exit 1", "replay"])
            .arg(fixture)
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().expect("replay process");
        let info = crate::watcher::watch_streaming(
            &crate::agent::cursor::CursorAgent, &mut child, &task.id, &store,
            &crate::paths::log_path(task.id.as_str()), None,
            crate::idle_timeout::DEFAULT_IDLE_TIMEOUT, None,
        ).await.expect("finished watcher");
        assert_eq!(info.status, crate::types::TaskStatus::Failed);
        assert_eq!(info.exit_code, Some(1));
        let content = std::fs::read_to_string(rate_limit::group_marker_path(&AgentKind::Cursor, None, "auto"))
            .expect("watcher must write the Auto hold");
        let expected = chrono::DateTime::parse_from_rfc3339("2026-09-24T00:00:00Z")
            .expect("expected reset").with_timezone(&Local).naive_local();
        assert_eq!(rate_limit::marker_field(&content, "recovery_at: "), Some(rate_limit::format_recovery(expected)));
        assert!(content.contains(CAPTURE.trim()), "{content}");
        assert!(!rate_limit::marker_path(&AgentKind::Cursor, None).exists());
        assert!(!rate_limit::group_marker_path(&AgentKind::Cursor, None, "premium").exists());
    }

    #[test]
    fn captured_cursor_refusal_classifies_finished_output_and_writes_cycle_hold() {
        let temp = tempfile::tempdir().expect("temp home");
        let _home = crate::paths::AidHomeGuard::set(temp.path());
        let _cache = crate::live_quota::CacheDirGuard::set(temp.path());
        crate::paths::ensure_dirs().expect("aid directories");
        let task = TaskId("t-cursor-monthly-capture".to_string());
        std::fs::write(crate::paths::stderr_path(task.as_str()), CAPTURE).expect("stderr");
        let refusal = crate::cmd::run::read_quota_error_message(&task, &AgentKind::Cursor)
            .expect("finished task must classify as a quota failure");
        assert_eq!(refusal, CAPTURE.trim());
        assert_eq!(
            record_quota_exhaustion(&refusal, AgentKind::Cursor, None, Some("Auto")),
            QuotaOutcome::RecordedFailed,
        );
        let marker = rate_limit::group_marker_path(&AgentKind::Cursor, None, "auto");
        let content = std::fs::read_to_string(marker).expect("Auto hold marker");
        let expected = chrono::DateTime::parse_from_rfc3339("2026-09-24T00:00:00Z")
            .expect("expected reset").with_timezone(&Local).naive_local();
        assert_eq!(
            rate_limit::marker_field(&content, "recovery_at: "),
            Some(rate_limit::format_recovery(expected)),
        );
        assert!(content.contains(CAPTURE.trim()), "{content}");
        assert!(!rate_limit::marker_path(&AgentKind::Cursor, None).exists());
        assert!(!rate_limit::group_marker_path(&AgentKind::Cursor, None, "premium").exists());
    }

    #[test]
    fn cursor_monthly_hold_blocks_only_the_dispatched_model_group() {
        let temp = tempfile::tempdir().expect("temp home");
        let _home = crate::paths::AidHomeGuard::set(temp.path());
        let _cache = crate::live_quota::CacheDirGuard::set(temp.path());
        let refusal = CAPTURE.replace("9/23/2026", "9/23/2099");
        for (model, group, other) in [("Auto", "auto", "composer-2.5"), ("composer-2.5", "premium", "auto")] {
            assert_eq!(record_quota_exhaustion(&refusal, AgentKind::Cursor, None, Some(model)), QuotaOutcome::RecordedFailed);
            assert!(rate_limit::is_group_rate_limited(&AgentKind::Cursor, None, group));
            assert!(rate_limit::dispatch_blocking_hold_for_model(&AgentKind::Cursor, None, Some(model)).is_some());
            assert!(rate_limit::dispatch_blocking_hold_for_model(&AgentKind::Cursor, None, Some(other)).is_none());
            assert!(!rate_limit::is_rate_limited(&AgentKind::Cursor, None));
            rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Cursor, None);
        }
    }

    #[test]
    fn cursor_missing_or_invalid_cycle_date_holds_for_thirty_days() {
        let temp = tempfile::tempdir().expect("temp home");
        let _home = crate::paths::AidHomeGuard::set(temp.path());
        let _cache = crate::live_quota::CacheDirGuard::set(temp.path());
        for date in ["", "not-a-date", "2/30/2099", "13/1/2099", "9/23/26"] {
            let refusal = CAPTURE.replace("9/23/2026", date);
            assert_eq!(record_quota_exhaustion(&refusal, AgentKind::Cursor, None, Some("auto")), QuotaOutcome::RecordedFailed);
            let content = std::fs::read_to_string(rate_limit::group_marker_path(&AgentKind::Cursor, None, "auto")).expect("marker");
            let recovery = rate_limit::marker_field(&content, "recovery_at: ").expect("long hold");
            let at = rate_limit::parse_recovery_datetime(&recovery).expect("valid recovery");
            let minutes = (at - Local::now().naive_local()).num_minutes();
            assert!((43_198..=43_200).contains(&minutes), "{content}");
            assert!(rate_limit::is_group_rate_limited(&AgentKind::Cursor, None, "auto"));
        }
    }

    #[test]
    fn cursor_refusal_requires_error_line_and_rejects_agent_authored_envelopes() {
        let temp = tempfile::tempdir().expect("temp home");
        let _home = crate::paths::AidHomeGuard::set(temp.path());
        for prose in [
            CAPTURE.replace("ActionRequiredError: ", ""),
            format!("The CLI reported: {CAPTURE}"),
            format!("> {CAPTURE}"),
            serde_json::json!({"type":"assistant", "message":{"content":[{"type":"text","text":CAPTURE}]}}).to_string(),
            serde_json::json!({"type":"tool_result", "is_error":true,"content":CAPTURE}).to_string(),
            serde_json::json!({"type":"result", "result":CAPTURE}).to_string(),
        ] {
            assert_eq!(record_quota_exhaustion(&prose, AgentKind::Cursor, None, Some("auto")), QuotaOutcome::None, "{prose}");
        }
        assert!(!rate_limit::group_marker_path(&AgentKind::Cursor, None, "auto").exists());
        assert_eq!(crate::rate_limit_signatures::match_quota_signature(CAPTURE), Some((AgentKind::Cursor, crate::rate_limit_signatures::QuotaRecovery::After(43_200))));
    }

    #[test]
    fn cursor_cycle_end_handles_single_digit_dates_and_leap_days() {
        for (date, expected) in [("1/2/2099", "2099-01-03T00:00:00Z"), ("2/29/2096", "2096-03-01T00:00:00Z")] {
            let refusal = CAPTURE.replace("9/23/2026", date).to_lowercase();
            let expected = chrono::DateTime::parse_from_rfc3339(expected)
                .expect("expected reset").with_timezone(&Local).naive_local();
            assert_eq!(cycle_end(&refusal), Some(expected));
        }
    }
}

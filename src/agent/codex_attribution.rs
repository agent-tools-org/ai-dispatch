// Codex session-file model attribution after JSONL completion.
// Exports: grade_completion_observation for the shared completion paths.
// Deps: Codex rollout files, Store task session IDs, and attribution types.

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local};

use crate::agent::Agent;
use crate::store::Store;
use crate::types::{
    grade_observation, AgentKind, AttributionSource, CompletionInfo, TaskId, TaskStatus,
};

pub(crate) fn grade_completion_observation(
    agent: &dyn Agent,
    store: &Store,
    task_id: &TaskId,
    info: &CompletionInfo,
    requested: Option<&str>,
) -> (Option<String>, Option<AttributionSource>) {
    let succeeded = info.status == TaskStatus::Done;
    if info.model.is_some() {
        return grade_observation(info.model.as_deref(), requested, succeeded);
    }
    if agent.kind() == AgentKind::Codex {
        return observed_model_for_task(store, task_id)
            .map_or((None, None), |model| {
                (Some(model), Some(AttributionSource::RecordedByCli))
            });
    }
    grade_observation(None, requested, succeeded)
}

fn observed_model_for_task(store: &Store, task_id: &TaskId) -> Option<String> {
    let task = store.get_task(task_id.as_str()).ok().flatten()?;
    let thread_id = task.agent_session_id.as_deref()?;
    observed_model_for_thread(&codex_home(), thread_id, task.created_at)
}

fn codex_home() -> PathBuf {
    resolve_codex_home(std::env::var_os("CODEX_HOME"), std::env::var_os("HOME"))
}

fn resolve_codex_home(codex_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    if let Some(path) = codex_home {
        return PathBuf::from(path);
    }
    PathBuf::from(home.unwrap_or_else(|| OsString::from("."))).join(".codex")
}

fn observed_model_for_thread(
    codex_home: &Path,
    thread_id: &str,
    created_at: DateTime<Local>,
) -> Option<String> {
    let session_path = find_session_file(codex_home, thread_id, created_at)?;
    for line in BufReader::new(fs::File::open(session_path).ok()?).lines().flatten() {
        if let Some(model) = model_from_turn_context(&line) {
            return Some(model);
        }
    }
    None
}

fn find_session_file(
    codex_home: &Path,
    thread_id: &str,
    created_at: DateTime<Local>,
) -> Option<PathBuf> {
    find_session_file_in_day(codex_home, thread_id, created_at).or_else(|| {
        find_session_file_in_day(codex_home, thread_id, created_at + Duration::days(1))
    })
}

fn find_session_file_in_day(
    codex_home: &Path,
    thread_id: &str,
    date: DateTime<Local>,
) -> Option<PathBuf> {
    let day = codex_home
        .join("sessions")
        .join(date.format("%Y/%m/%d").to_string());
    for entry in fs::read_dir(day).ok()?.flatten() {
        let path = entry.path();
        if session_file_matches(&path, thread_id) {
            return Some(path);
        }
    }
    None
}

fn session_file_matches(path: &Path, thread_id: &str) -> bool {
    super::rollout_filename_matches(path, thread_id)
}

fn model_from_turn_context(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if !is_turn_context_value(&value) {
        return None;
    }
    value
        .pointer("/payload/model")
        .and_then(serde_json::Value::as_str)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
}

fn is_turn_context_value(value: &serde_json::Value) -> bool {
    value.get("type").and_then(serde_json::Value::as_str) == Some("turn_context")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
    use tempfile::tempdir;

    fn test_date() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, 9, 12, 0, 0)
            .single()
            .expect("valid test date")
    }

    #[test]
    fn resolves_codex_home_before_home_default() {
        assert_eq!(
            resolve_codex_home(
                Some(OsString::from("/custom/codex")),
                Some(OsString::from("/home/user")),
            ),
            PathBuf::from("/custom/codex")
        );
        assert_eq!(
            resolve_codex_home(None, Some(OsString::from("/home/user"))),
            PathBuf::from("/home/user/.codex")
        );
    }

    #[test]
    fn reads_model_from_turn_context_after_modelless_session_meta() {
        let home = tempdir().expect("temp home");
        let created_at = test_date();
        let day = home
            .path()
            .join("sessions")
            .join(created_at.format("%Y/%m/%d").to_string());
        fs::create_dir_all(&day).expect("session directory");
        let thread_id = "019fe4ce-9cf4-79f1-b7e8-b32831ca775d";
        let path = day.join(format!("rollout-2026-08-09T00-00-00-{thread_id}.jsonl"));
        fs::write(
            path,
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"openai\"}}\n{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-luna\"}}\n",
        )
        .expect("session metadata");

        assert_eq!(
            observed_model_for_thread(home.path(), thread_id, created_at).as_deref(),
            Some("gpt-5.6-luna")
        );
    }

    #[test]
    fn later_turn_context_model_is_used_after_modeless_turn() {
        let home = tempdir().expect("temp home");
        let created_at = test_date();
        let day = home.path().join("sessions/2026/08/09");
        fs::create_dir_all(&day).expect("session directory");
        let thread_id = "019fe4ce-9cf4-79f1-b7e8-b32831ca775d";
        let path = day.join(format!("rollout-2026-08-09T00-00-00-{thread_id}.jsonl"));
        fs::write(
            path,
            "{\"type\":\"turn_context\",\"payload\":{}}\n{\"type\":\"turn_context\",\"payload\":{\"model\":\"later-model\"}}\n",
        )
        .expect("session metadata");

        assert_eq!(
            observed_model_for_thread(home.path(), thread_id, created_at).as_deref(),
            Some("later-model")
        );
    }

    #[test]
    fn session_with_no_model_in_any_turn_context_stays_unknown() {
        let home = tempdir().expect("temp home");
        let created_at = test_date();
        let day = home.path().join("sessions/2026/08/09");
        fs::create_dir_all(&day).expect("session directory");
        let thread_id = "019fe4ce-9cf4-79f1-b7e8-b32831ca775d";
        let path = day.join(format!("rollout-2026-08-09T00-00-00-{thread_id}.jsonl"));
        fs::write(
            path,
            "{\"type\":\"turn_context\",\"payload\":{\"effort\":\"high\"}}\n{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"/tmp\"}}\n",
        )
        .expect("session metadata");

        assert_eq!(observed_model_for_thread(home.path(), thread_id, created_at), None);
    }

    #[test]
    fn rollout_lookup_uses_created_day_then_next_day_only() {
        let home = tempdir().expect("temp home");
        let created_at = Local
            .with_ymd_and_hms(2026, 8, 9, 23, 59, 59)
            .single()
            .expect("valid midnight-edge test date");
        let next_day = home.path().join("sessions/2026/08/10");
        fs::create_dir_all(&next_day).expect("session directory");
        let thread_id = "019fe4ce-9cf4-79f1-b7e8-b32831ca775d";
        let path = next_day.join(format!("rollout-2026-08-10T00-00-01-{thread_id}.jsonl"));
        fs::write(path, "session").expect("session file");

        assert!(find_session_file(home.path(), thread_id, created_at).is_some());
        assert_eq!(
            find_session_file(
                home.path(),
                thread_id,
                created_at - Duration::days(1),
            ),
            None
        );
    }

    #[test]
    fn missing_or_malformed_session_metadata_stays_unknown() {
        assert_eq!(model_from_turn_context("not json"), None);
        assert_eq!(
            model_from_turn_context("{\"payload\":{\"model\":\"gpt-5.6-luna\"}}"),
            None
        );
        assert!(!session_file_matches(
            Path::new("rollout-2026-08-09T00-00-00-other.jsonl"),
            "thread-id"
        ));
        assert!(!session_file_matches(
            Path::new(
                "rollout-2026-08-09T00-00-00-extra-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl"
            ),
            "019e3e49-6b83-7563-a3d8-b51a3a716dd1"
        ));
        assert!(session_file_matches(
            Path::new("rollout-2026-08-09T00-00-00-session-123.jsonl"),
            "session-123"
        ));
    }

    #[test]
    fn codex_without_rollout_stays_unknown_instead_of_confirming_request() {
        let store = Store::open_memory().expect("in-memory store");
        let info = CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        };

        assert_eq!(
            grade_completion_observation(
                &crate::agent::codex::CodexAgent,
                &store,
                &TaskId("missing-task".to_string()),
                &info,
                Some("gpt-5.6-luna"),
            ),
            (None, None)
        );
    }
}

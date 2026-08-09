// Codex session-file model attribution after JSONL completion.
// Exports: grade_completion_observation for the shared completion paths.
// Deps: Codex rollout files, Store task session IDs, and attribution types.

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

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
    observed_model_for_thread(&codex_home(), thread_id)
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

fn observed_model_for_thread(codex_home: &Path, thread_id: &str) -> Option<String> {
    let session_path = find_session_file(codex_home, thread_id)?;
    for line in BufReader::new(fs::File::open(session_path).ok()?).lines().flatten() {
        if is_turn_context(&line) {
            return model_from_turn_context(&line);
        }
    }
    None
}

fn find_session_file(codex_home: &Path, thread_id: &str) -> Option<PathBuf> {
    let sessions = codex_home.join("sessions");
    let years = fs::read_dir(sessions).ok()?;
    for year in years.flatten() {
        let year = year.path();
        if !year.is_dir() {
            continue;
        }
        let Ok(months) = fs::read_dir(year) else {
            continue;
        };
        for month in months.flatten() {
            let month = month.path();
            if !month.is_dir() {
                continue;
            }
            let Ok(days) = fs::read_dir(month) else {
                continue;
            };
            for day in days.flatten() {
                let day = day.path();
                if !day.is_dir() {
                    continue;
                }
                let Ok(entries) = fs::read_dir(day) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if session_file_matches(&path, thread_id) {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

fn session_file_matches(path: &Path, thread_id: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("rollout-") && name.ends_with(&format!("-{thread_id}.jsonl"))
        })
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

fn is_turn_context(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .is_some_and(|value| is_turn_context_value(&value))
}

fn is_turn_context_value(value: &serde_json::Value) -> bool {
    value.get("type").and_then(serde_json::Value::as_str) == Some("turn_context")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
        let day = home.path().join("sessions/2026/08/09");
        fs::create_dir_all(&day).expect("session directory");
        let thread_id = "019fe4ce-9cf4-79f1-b7e8-b32831ca775d";
        let path = day.join(format!("rollout-2026-08-09T00-00-00-{thread_id}.jsonl"));
        fs::write(
            path,
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"openai\"}}\n{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-luna\"}}\n",
        )
        .expect("session metadata");

        assert_eq!(
            observed_model_for_thread(home.path(), thread_id).as_deref(),
            Some("gpt-5.6-luna")
        );
    }

    #[test]
    fn first_turn_context_without_model_stays_unknown() {
        let home = tempdir().expect("temp home");
        let day = home.path().join("sessions/2026/08/09");
        fs::create_dir_all(&day).expect("session directory");
        let thread_id = "019fe4ce-9cf4-79f1-b7e8-b32831ca775d";
        let path = day.join(format!("rollout-2026-08-09T00-00-00-{thread_id}.jsonl"));
        fs::write(
            path,
            "{\"type\":\"turn_context\",\"payload\":{}}\n{\"type\":\"turn_context\",\"payload\":{\"model\":\"later-model\"}}\n",
        )
        .expect("session metadata");

        assert_eq!(observed_model_for_thread(home.path(), thread_id), None);
    }

    #[test]
    fn missing_or_malformed_session_metadata_stays_unknown() {
        assert_eq!(model_from_turn_context("not json"), None);
        assert_eq!(
            is_turn_context("{\"type\":\"session_meta\",\"payload\":{\"model\":\"gpt-5.6-luna\"}}"),
            false
        );
        assert_eq!(
            model_from_turn_context("{\"payload\":{\"model\":\"gpt-5.6-luna\"}}"),
            None
        );
        assert!(!session_file_matches(
            Path::new("rollout-2026-08-09T00-00-00-other.jsonl"),
            "thread-id"
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

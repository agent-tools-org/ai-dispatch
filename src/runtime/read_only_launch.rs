// Persist native read-only permissions at the executed launch boundary.
// Settlement consumes the latest launch, including explicit unknown/reset records.
// Deps: Codex command translation, canonical root validation and Store events.

use crate::{
    store::Store,
    types::{EventKind, TaskEvent, TaskId},
};
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    identity: String,
    base: String,
    writable_roots: Vec<PathBuf>,
    approval_policy: String,
    cwd: PathBuf,
}

fn profile(cmd: &Command) -> Option<Profile> {
    let (config, cwd) = super::codex_app_server::command::read_only_config(cmd)?;
    if config["default_permissions"] != "aid-read-only" || config["approval_policy"] != "never" {
        return None;
    }
    let permissions = config["permissions"].as_object()?;
    if permissions.len() != 1 {
        return None;
    }
    let policy = permissions.get("aid-read-only")?.as_object()?;
    if policy.len() != 2 || policy.get("extends")? != ":read-only" {
        return None;
    }
    let filesystem = policy.get("filesystem")?.as_object()?;
    let mut roots = Vec::new();
    for (path, access) in filesystem {
        if access != "write" {
            return None;
        }
        roots.push(PathBuf::from(path));
    }
    let canonical = crate::agent::scratch::validate_read_only_roots(cwd.to_str(), &roots).ok()?;
    if roots != canonical {
        return None;
    }
    Some(Profile {
        identity: "aid-read-only".into(),
        base: ":read-only".into(),
        writable_roots: canonical,
        approval_policy: "never".into(),
        cwd,
    })
}

pub(crate) fn record(store: &Store, task: &TaskId, cmd: &Command, transport: &str) -> Result<()> {
    write(
        store,
        task,
        serde_json::json!({
            "transport": transport, "profile": profile(cmd),
        }),
    )
}

pub(crate) fn reset(store: &Store, task: &TaskId) -> Result<()> {
    write(store, task, serde_json::Value::Null)
}

fn write(store: &Store, task: &TaskId, launch: serde_json::Value) -> Result<()> {
    store.insert_agent_event(&TaskEvent {
        task_id: task.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Setup,
        detail: "Read-only launch provenance".into(),
        metadata: Some(serde_json::json!({"read_only_launch": launch})),
    })
}

pub(crate) fn enforced(store: &Store, task: &TaskId, dir: &str) -> Result<bool> {
    let metadata: Option<String> = store
        .db()
        .query_row(
            "SELECT metadata FROM events WHERE task_id = ?1 AND event_type = 'setup'
         AND json_type(metadata, '$.read_only_launch') IS NOT NULL ORDER BY id DESC LIMIT 1",
            [task.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(metadata) = metadata else {
        return Ok(false);
    };
    let value: serde_json::Value = serde_json::from_str(&metadata)?;
    let launch = &value["read_only_launch"];
    if !matches!(launch["transport"].as_str(), Some("exec" | "app-server")) {
        return Ok(false);
    }
    let Ok(profile) = serde_json::from_value::<Profile>(launch["profile"].clone()) else {
        return Ok(false);
    };
    Ok(profile.identity == "aid-read-only"
        && profile.base == ":read-only"
        && profile.approval_policy == "never"
        && profile.cwd == Path::new(dir).canonicalize()?
        && profile.writable_roots.iter().all(|root| {
            root.is_absolute() && !root.starts_with(&profile.cwd) && !profile.cwd.starts_with(root)
        }))
}

#[cfg(test)]
#[path = "read_only_launch_tests.rs"]
mod tests;

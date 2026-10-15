// Recreate the temporary home path recorded by Codex for an aid-owned session.
// Holds the originating task's cleanup lease without editing Codex's shared database.

use super::{cleanup, command, home};
use crate::{store::Store, types::TaskId};
use anyhow::Result;
use rusqlite::OptionalExtension;
use std::process::Command;

pub(super) fn origin(store: &Store, session: &str) -> Result<Option<String>> {
    Ok(store
        .db()
        .query_row(
            "SELECT id FROM tasks WHERE agent_session_id=?1 ORDER BY created_at, rowid LIMIT 1",
            [session],
            |row| row.get(0),
        )
        .optional()?)
}

pub(crate) struct ExecFallback {
    pub command: Command,
    _home: Option<home::Home>,
    _owner: Option<cleanup::Owner>,
}

impl ExecFallback {
    pub fn command(&self) -> Command {
        clone_command(&self.command, false)
    }
}

pub(crate) fn exec_fallback(cmd: &Command, store: &Store, task: &TaskId) -> Result<ExecFallback> {
    let launch = command::translate(cmd).ok();
    let refused = store.get_events(task.as_str())?.iter().any(|event| {
        event.metadata.as_ref().is_some_and(|metadata| {
            metadata["codex_app_server_fallback"] == true
                && metadata["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("Codex thread/resume:"))
        })
    });
    let mut command = clone_command(cmd, refused);
    let mut home = None;
    let mut owner = None;
    if let Some(launch) = launch.filter(|launch| launch.resume && launch.isolated && !refused) {
        match restore(
            cmd,
            store,
            launch.thread["threadId"].as_str().unwrap_or_default(),
        ) {
            Ok(Some((restored_home, restored_owner))) => {
                home = Some(restored_home);
                owner = Some(restored_owner);
            }
            Ok(None) => {}
            Err(error) => {
                command = clone_command(cmd, true);
                store.insert_event(&crate::types::TaskEvent {
                    task_id: task.clone(),
                    timestamp: chrono::Local::now(),
                    event_kind: crate::types::EventKind::Milestone,
                    detail: format!("Codex exec resume unavailable; starting fresh: {error:#}"),
                    metadata: None,
                })?;
            }
        }
    }
    Ok(ExecFallback {
        command,
        _home: home,
        _owner: owner,
    })
}

fn restore(
    cmd: &Command,
    store: &Store,
    session: &str,
) -> Result<Option<(home::Home, cleanup::Owner)>> {
    let Some(origin) = origin(store, session)? else {
        return Ok(None);
    };
    let dir = crate::support::paths::task_dir(&origin);
    std::fs::create_dir_all(&dir)?;
    let owner = cleanup::Owner::create(&origin)?;
    let home = home::Home::for_command(cmd, &dir)?;
    Ok(Some((home, owner)))
}

fn clone_command(cmd: &Command, fresh: bool) -> Command {
    let mut command = Command::new(cmd.get_program());
    let args: Vec<_> = cmd.get_args().collect();
    let mut skip_session = fresh && args.get(1).is_some_and(|arg| *arg == "resume");
    let mut option_value = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_session && index == 1 {
            continue;
        }
        if skip_session && index > 1 && !option_value && !arg.to_string_lossy().starts_with('-') {
            skip_session = false;
            continue;
        }
        const OPTIONS: &[&str] = &[
            "-c",
            "--config",
            "-m",
            "--model",
            "-o",
            "--output-last-message",
            "-C",
            "--cd",
        ];
        option_value = !option_value && arg.to_str().is_some_and(|arg| OPTIONS.contains(&arg));
        command.arg(arg);
    }
    if let Some(dir) = cmd.get_current_dir() {
        command.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    command
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;

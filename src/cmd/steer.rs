// Handler for `aid steer` — inject guidance into running PTY tasks.
// Delegates to persisted reply delivery with steer source tracking.

use anyhow::Result;

use crate::cmd::reply::{self, InputCommand};
use crate::store::Store;
use crate::types::MessageSource;

pub fn run(store: &Store, task_id: &str, message: &str) -> Result<()> {
    let app_server = crate::runtime::codex_app_server::active(task_id);
    let claude = crate::runtime::claude_input::active(task_id);
    let outcome = reply::run_with_source(
        store,
        task_id,
        Some(message),
        None,
        !(app_server || claude),
        30,
        MessageSource::Steer,
        InputCommand::Steer,
    )?;
    match outcome {
        reply::ReplyOutcome::ClaudeAccepted => println!("accepted by claude"),
        reply::ReplyOutcome::TimedOut if claude => {
            anyhow::bail!("Claude input sent or queued, acceptance unconfirmed after timeout")
        }
        reply::ReplyOutcome::Accepted { turn_id } => println!("accepted by codex (turn {turn_id})"),
        reply::ReplyOutcome::TimedOut if app_server => {
            anyhow::bail!("Codex steer response unconfirmed after timeout")
        }
        _ => println!(
            "Queued steer for {task_id}: {}",
            message.chars().take(80).collect::<String>()
        ),
    }
    Ok(())
}

pub(crate) fn wrap_up_before_timeout(
    store: &Store,
    task_id: &str,
    elapsed_secs: u64,
    cap_secs: u64,
) -> Result<bool> {
    if cap_secs < 600 || elapsed_secs < cap_secs - cap_secs / 10 || elapsed_secs > cap_secs {
        return Ok(false);
    }
    let Some(task) = store.get_task(task_id)? else {
        return Ok(false);
    };
    if reply::ensure_interactive_input(&task, InputCommand::Steer).is_err()
        || crate::support::paths::steer_signal_path(task_id).exists()
    {
        return Ok(false);
    }
    queue_wrap_up(store, task_id, task.read_only)
}

fn wrap_up_needed(conn: &rusqlite::Connection, task_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1
         AND status IN ('running', 'awaiting_input', 'stalled')
         AND NOT EXISTS (SELECT 1 FROM events WHERE task_id = ?1 AND
             CASE WHEN json_valid(metadata)
             THEN json_extract(metadata, '$.hard_cap_wrap_up') END = 1))",
        [task_id],
        |row| row.get(0),
    )?)
}

fn queue_wrap_up(store: &Store, task_id: &str, read_only: bool) -> Result<bool> {
    let mut conn = store.db();
    if !wrap_up_needed(&conn, task_id)?
        || crate::support::paths::steer_signal_path(task_id).exists()
    {
        return Ok(false);
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if crate::support::paths::steer_signal_path(task_id).exists() {
        return Ok(false);
    }
    let (message, detail) = if read_only {
        (
            "Hard timeout is approaching: stop investigating and write the report now with the evidence \
          gathered so far and the open questions. Write only an already authorized result file; \
          this request grants no new write permission.",
            "Hard-cap wrap-up: queued request to stop investigating and write the report",
        )
    } else {
        (
            "Hard timeout is approaching: please commit work in progress if changes are permitted, \
                   and write the report now.",
            "Hard-cap wrap-up: queued request to commit work in progress and write the report",
        )
    };
    let rows = tx.execute(
        "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         SELECT id, ?2, 'milestone', ?3, ?4 FROM tasks WHERE id = ?1
         AND status IN ('running', 'awaiting_input', 'stalled')
         AND NOT EXISTS (SELECT 1 FROM events WHERE task_id = ?1 AND
             CASE WHEN json_valid(metadata)
             THEN json_extract(metadata, '$.hard_cap_wrap_up') END = 1)",
        rusqlite::params![
            task_id,
            chrono::Local::now().to_rfc3339(),
            detail,
            r#"{"input_delivery":"queued","hard_cap_wrap_up":true}"#
        ],
    )?;
    if rows == 0 {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO task_messages (task_id, direction, content, source, created_at)
         VALUES (?1, 'in', ?2, 'steer', ?3)",
        rusqlite::params![task_id, message, chrono::Local::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(true)
}

#[cfg(test)]
#[path = "steer_tests.rs"]
mod tests;

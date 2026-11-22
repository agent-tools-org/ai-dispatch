// Wakes the Claude Code session that dispatched a task once the task settles.
// Captures the session's messaging socket at startup and removes it from the environment,
// so agents never inherit it; pushes fixed aid fields only, never agent output.
use anyhow::Result;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::store::Store;
use crate::types::Task;

const SOCKET_VAR: &str = "CLAUDE_CODE_MESSAGING_SOCKET";
const TOKEN_VAR: &str = "CLAUDE_CODE_MESSAGING_TOKEN";
const PENDING_LIMIT: usize = 20;

/// The dispatching session's inbox, as Claude Code exported it to this process.
#[derive(Clone, Debug)]
pub(crate) struct Channel {
    pub socket: PathBuf,
    pub token: String,
    pub session: String,
}

static CHANNEL: OnceLock<Option<Channel>> = OnceLock::new();

/// Takes the socket and token out of the environment. Must run first in `main`,
/// before any thread exists, because it mutates the process environment.
pub(crate) fn capture_env() {
    let socket = std::env::var_os(SOCKET_VAR);
    let token = std::env::var(TOKEN_VAR).ok();
    // SAFETY: called at the top of main, before the runtime or any thread starts.
    unsafe {
        std::env::remove_var(SOCKET_VAR);
        std::env::remove_var(TOKEN_VAR);
    }
    // Claude Code sets this itself; AID_CALLER_SESSION can be overridden, so it
    // never decides which socket a task's notice goes to.
    let session = std::env::var("CLAUDE_CODE_SESSION_ID").ok().filter(|id| !id.is_empty());
    let channel = match (socket, token, session) {
        (Some(socket), Some(token), Some(session)) if !token.is_empty() => {
            Some(Channel { socket: PathBuf::from(socket), token, session })
        }
        _ => None,
    };
    let _ = CHANNEL.set(channel);
}

fn channel() -> Option<&'static Channel> {
    CHANNEL.get().and_then(Option::as_ref)
}

/// Variables the background worker needs to push for its caller. Only aid's own
/// worker gets them; agents, verifiers and hooks it starts inherit nothing.
#[cfg_attr(test, allow(dead_code))] // the worker spawner is compiled out of unit tests
pub(crate) fn worker_env() -> Vec<(&'static str, OsString)> {
    channel().map_or_else(Vec::new, |channel| {
        vec![
            (SOCKET_VAR, channel.socket.clone().into_os_string()),
            (TOKEN_VAR, OsString::from(&channel.token)),
        ]
    })
}

/// Records a settled task for its caller and pushes once when this process can
/// reach that caller. Never fails the task: errors are reported and the record
/// stays pending for the caller's next session start.
pub(crate) fn on_terminal(store: &Store, task: &Task) {
    if let Err(err) = deliver(store, task, channel()) {
        aid_warn!("[aid] Caller notification for {} not delivered: {err}", task.id);
    }
}

pub(crate) fn deliver(store: &Store, task: &Task, channel: Option<&Channel>) -> Result<()> {
    let Some(session) = task.caller_session_id.as_deref() else {
        return Ok(());
    };
    // Done without completed_at is still verifying; it is not news yet.
    if !task.status.is_terminal() || task.completed_at.is_none() {
        return Ok(());
    }
    let id = task.id.as_str();
    store.inbox_record(id, session)?;
    let Some(channel) = channel.filter(|channel| channel.session == session) else {
        return Ok(());
    };
    if !store.inbox_claim_push(id)? {
        return Ok(());
    }
    if let Err(err) = send(channel, &message(task)) {
        store.inbox_release_push(id)?;
        return Err(err);
    }
    Ok(())
}

/// Records a stopped task as already read by whoever stopped it; a session that
/// stops its own task does not need to be woken about it.
pub(crate) fn on_stopped(store: &Store, task: &Task) {
    if let Err(err) = deliver(store, task, None) {
        aid_warn!("[aid] Caller record for {} not written: {err}", task.id);
    }
    mark_seen(store, task.id.as_str());
}

/// Fixed fields only: the agent's prompt and output never reach the caller this way.
pub(crate) fn message(task: &Task) -> String {
    let verify = match task.verify_status.as_str() {
        "skipped" | "pending" | "" => String::new(),
        status => format!(", verify {status}"),
    };
    format!(
        "[aid] Task {id} settled: {label} ({outcome}{verify}), agent {agent}. \
         It was dispatched from this session. Read it with `aid show {id}`.",
        id = task.id,
        label = task.status.label(),
        outcome = task.outcome().as_str(),
        agent = plain_name(&task.agent_display_name()),
    )
}

/// Custom agent names come from config files; keep them to a short plain token.
fn plain_name(name: &str) -> String {
    let plain: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(40)
        .collect();
    if plain.is_empty() { "custom".into() } else { plain }
}

/// Bounds the whole push, connect included, so a stuck inbox never holds a task.
#[cfg(unix)]
fn send(channel: &Channel, text: &str) -> Result<()> {
    let (channel, text) = (channel.clone(), text.to_string());
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(write_notice(&channel, &text));
    });
    result
        .recv_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| anyhow::anyhow!("caller inbox did not accept the notice within 3s"))?
}

#[cfg(unix)]
fn write_notice(channel: &Channel, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(&channel.socket)?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
    let auth = serde_json::json!({ "type": "auth", "token": channel.token });
    let user = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": text },
    });
    writeln!(stream, "{auth}")?;
    writeln!(stream, "{user}")?;
    stream.shutdown(std::net::Shutdown::Write)?;
    Ok(())
}

#[cfg(not(unix))]
fn send(_channel: &Channel, _text: &str) -> Result<()> {
    anyhow::bail!("caller push is only implemented for Unix sockets")
}

/// Acknowledges a task when the session that dispatched it reads it.
pub(crate) fn mark_seen(store: &Store, task_id: &str) {
    if let Some(caller) = crate::runtime::session::current_caller() {
        let _ = store.inbox_mark_seen(task_id, &caller.session_id);
    }
}

/// Session-start notice listing settled tasks this session has not read.
pub(crate) fn pending_notice(store: &Store, session: &str) -> Option<String> {
    let pending = store.inbox_pending(session, PENDING_LIMIT).ok()?;
    if pending.is_empty() {
        return None;
    }
    let list = pending
        .iter()
        .map(|item| format!("{} ({})", item.task_id, item.status))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "[aid] Tasks dispatched from this session settled while it was away and are unread: \
         {list}. Read each with `aid show <id>`; reading one clears it."
    ))
}

#[cfg(test)]
#[path = "caller_wake_tests.rs"]
mod tests;

// Wakes the Claude Code session that dispatched a task once the task settles.
// Removes the session's messaging socket from aid's environment at startup; only the
// worker running a task pushes, once, with fixed aid fields and never agent output.
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
pub(crate) struct Inbox {
    pub socket: PathBuf,
    pub token: String,
}

/// Only the background worker pushes, and only for the task it runs: it got the
/// socket straight from the process that dispatched that task, so no session
/// matching is needed and no other process can send for someone else's task.
#[derive(Clone, Debug)]
pub(crate) struct Channel {
    pub inbox: Inbox,
    pub task_id: String,
}

static CAPTURED: OnceLock<Option<Inbox>> = OnceLock::new();
static WORKER_TASK: OnceLock<Option<String>> = OnceLock::new();

/// Takes the socket and token out of the environment. Must run first in `main`,
/// before any thread exists, because it mutates the process environment.
/// `worker_task` is set only in the background worker process.
pub(crate) fn capture_env(worker_task: Option<String>) {
    let socket = std::env::var_os(SOCKET_VAR);
    let token = std::env::var(TOKEN_VAR).ok();
    // SAFETY: called at the top of main, before the runtime or any thread starts.
    unsafe {
        std::env::remove_var(SOCKET_VAR);
        std::env::remove_var(TOKEN_VAR);
    }
    let inbox = match (socket, token) {
        (Some(socket), Some(token)) if !token.is_empty() => {
            Some(Inbox { socket: PathBuf::from(socket), token })
        }
        _ => None,
    };
    let _ = CAPTURED.set(inbox);
    let _ = WORKER_TASK.set(worker_task);
}

fn channel() -> Option<Channel> {
    let inbox = CAPTURED.get()?.clone()?;
    let task_id = WORKER_TASK.get()?.clone()?;
    Some(Channel { inbox, task_id })
}

/// Variables the background worker needs to push for its caller. Only aid's own
/// worker gets them; agents, verifiers and hooks it starts inherit nothing.
#[cfg_attr(test, allow(dead_code))] // the worker spawner is compiled out of unit tests
pub(crate) fn worker_env() -> Vec<(&'static str, OsString)> {
    CAPTURED.get().and_then(Option::as_ref).map_or_else(Vec::new, |inbox| {
        vec![
            (SOCKET_VAR, inbox.socket.clone().into_os_string()),
            (TOKEN_VAR, OsString::from(&inbox.token)),
        ]
    })
}

/// Records a settled task for its caller and, in the worker running it, pushes once.
/// Never fails the task: errors are reported and the task stays unread, so the
/// caller's next session start lists it.
pub(crate) fn on_terminal(store: &Store, task: &Task) {
    if let Err(err) = deliver(store, task, channel().as_ref()) {
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
    let Some(channel) = channel.filter(|channel| channel.task_id == id) else {
        return Ok(());
    };
    // A claim is an attempt, never released: a late write cannot be followed by a second.
    if store.inbox_claim_push(id)? {
        send(&channel.inbox, &message(task))?;
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
fn send(inbox: &Inbox, text: &str) -> Result<()> {
    let (inbox, text) = (inbox.clone(), text.to_string());
    let (done, result) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("caller-wake".into()).spawn(move || {
        let _ = done.send(write_notice(&inbox, &text));
    })?;
    result
        .recv_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| anyhow::anyhow!("caller inbox did not accept the notice within 3s"))?
}

#[cfg(unix)]
fn write_notice(inbox: &Inbox, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(&inbox.socket)?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
    let auth = serde_json::json!({ "type": "auth", "token": inbox.token });
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
fn send(_inbox: &Inbox, _text: &str) -> Result<()> {
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

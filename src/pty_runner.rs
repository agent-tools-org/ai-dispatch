// PTY-backed agent execution for interactive background tasks.
// Runs combined stdout/stderr through prompt detection and file-based input forwarding.

use anyhow::Result;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;

use crate::agent::Agent;
use crate::cost;
use crate::pty_bridge::PtyBridge;
use crate::pty_watch::{MonitorState, finalize_output, monitor_bridge};
use crate::store::Store;
use crate::types::{CompletionInfo, TaskId};

#[allow(clippy::too_many_arguments)]
pub fn run_agent_process(
    agent: &dyn Agent,
    cmd: &std::process::Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut bridge = spawn_bridge(cmd)?;
    let rx = spawn_reader_thread(bridge.take_reader()?);
    let mut log_file = std::fs::File::create(log_path)?;
    let mut state = MonitorState::new();
    monitor_bridge(
        agent,
        task_id,
        store,
        &mut bridge,
        &rx,
        &mut log_file,
        &mut state,
        streaming,
    )?;
    let exit_status = bridge.wait()?;
    finalize_output(agent, task_id, store, output_path, streaming, &exit_status, &mut state)?;
    record_completion(agent, task_id, store, model, start.elapsed().as_millis() as i64, &state.info)
}

fn spawn_bridge(cmd: &std::process::Command) -> Result<PtyBridge> {
    let (argv, dir, env) = command_parts(cmd);
    PtyBridge::spawn(&argv, dir.as_deref(), env)
}

fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}

fn record_completion(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    model: Option<&str>,
    duration_ms: i64,
    info: &CompletionInfo,
) -> Result<()> {
    let final_model = info.model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| {
        info.tokens
            .and_then(|tokens| cost::estimate_cost(tokens, final_model, agent.kind()))
    });
    store.update_task_completion(
        task_id.as_str(),
        info.status,
        info.tokens,
        duration_ms,
        final_model,
        cost_usd,
    )?;
    println!(
        "Task {} {} ({}{}{})",
        task_id,
        info.status.label(),
        format_duration(duration_ms),
        info.tokens.map(|tokens| format!(", {} tokens", tokens)).unwrap_or_default(),
        cost_usd.map(|cost| format!(", {}", cost::format_cost(Some(cost)))).unwrap_or_default(),
    );
    Ok(())
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn command_parts(cmd: &std::process::Command) -> (Vec<String>, Option<String>, Vec<(String, String)>) {
    let argv = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let dir = cmd.get_current_dir().map(|path| path.to_string_lossy().into_owned());
    let env = cmd
        .get_envs()
        .filter_map(|(key, value)| Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned())))
        .collect();
    (argv, dir, env)
}

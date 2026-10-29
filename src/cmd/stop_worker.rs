// Verified worker and agent identity checks shared by all stop modes.
// A stop succeeds only after every recorded process is gone.
use anyhow::{Result, bail};
use crate::runtime::background::{self, BackgroundRunSpec};

pub(super) fn validate_live_identity(id: &str, spec: Option<&BackgroundRunSpec>) -> Result<()> {
    let Some(spec) = spec else {
        return Ok(());
    };
    if let Some(pid) = spec.worker_pid
        && background::is_process_running(pid)
        && (Some(pid) == spec.agent_pid || !BackgroundRunSpec::is_task_worker(pid, id))
    {
        bail!("Worker identity unverifiable for task {id}");
    }
    if let Some(pid) = spec.agent_pid
        && background::is_process_running(pid)
        && spec.agent_pid_for_signal().is_none()
    {
        bail!("Agent identity unverifiable for task {id}");
    }
    Ok(())
}

pub(super) fn confirm_exit(id: &str, spec: Option<&BackgroundRunSpec>) -> Result<()> {
    let Some(spec) = spec else {
        return Ok(());
    };
    for pid in [spec.worker_pid, spec.agent_pid].into_iter().flatten() {
        if super::wait_for_exit(pid) {
            bail!("Process {pid} still alive for task {id}");
        }
    }
    Ok(())
}

// Worker specs act as a barrier between process completion and settled results.
use super::*;

pub(super) fn worker_is_settling(task_id: &str) -> Result<bool> {
    let Some(spec) = crate::background::load_spec_if_exists(task_id)? else { return Ok(false); };
    if let Some(pid) = spec.worker_pid
        && !crate::background::is_process_running(pid)
    {
        // A worker can remove the spec and exit between the read and PID probe.
        if !crate::paths::job_path(task_id).try_exists()? { return Ok(false); }
        anyhow::bail!("Task {task_id}: worker exited before settlement completed");
    }
    Ok(true)
}

pub(super) fn include_worker_tasks(store: &Store, tasks: &mut Vec<Task>) -> Result<()> {
    // Done can still be provisional while the worker checks delivery artifacts.
    let jobs = match std::fs::read_dir(crate::paths::jobs_dir()) {
        Ok(jobs) => Some(jobs),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    for entry in jobs.into_iter().flatten() {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") { continue; }
        let Some(id) = path.file_stem().and_then(|id| id.to_str()) else { continue; };
        if !tasks.iter().any(|task| task.id.as_str() == id)
            && let Some(task) = store.get_task(id)?
        {
            tasks.push(task);
        }
    }
    Ok(())
}

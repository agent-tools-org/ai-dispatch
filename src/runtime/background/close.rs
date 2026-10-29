// Worker and reaper cleanup precede the shared final settlement predicate.
// Keeps the job spec as the existing evidence barrier when processes remain.
use super::*;
use super::spec::remove_spec;
use super::kill::terminate_task_processes;
use super::reaper::{record_failure, ZOMBIE_FAILURE_DETAIL};
pub(super) fn finish_worker(store: &Store, spec: &BackgroundRunSpec, result: Result<()>) -> Result<()> {
    let task_id = &spec.task_id;
    let mut evidence = load_spec_if_exists(task_id)?.unwrap_or_else(|| spec.clone());
    let captured = crate::runtime::execution_processes::capture(&mut evidence);
    if let Some(name) = &evidence.container_name {
        crate::runtime::container::stop_container(name);
    }
    let removed = remove_spec(task_id);
    let _ = crate::runtime::input_signal::clear_response(task_id);
    let _ = crate::runtime::input_signal::clear_steer(task_id);
    if result.is_ok()
        && let Some(cmd) = spec.on_done.as_ref()
    {
        let _ = spawn_on_done_command(cmd, task_id, "done");
    }
    removed?;
    captured?;
    crate::runtime::execution_close::close_warn(
        store,
        task_id,
        "completed",
        crate::runtime::execution_close::Closer::Worker,
        Some(&evidence),
    );
    result
}

pub(super) fn cleanup_dead_worker(
    store: &Store,
    task_id: &str,
    spec: &BackgroundRunSpec,
    worker_pid: Option<u32>,
    cleaned: &mut Vec<String>,
) -> Result<()> {
    let mut evidence = spec.clone();
    crate::runtime::execution_processes::capture(&mut evidence)?;
    super::reaper::preserve_zombie_changes(store, task_id, &evidence)?;
    terminate_task_processes(worker_pid, &evidence);
    if let Some(name) = &evidence.container_name {
        crate::runtime::container::stop_container(name);
    }
    let detail = format!(
        "{ZOMBIE_FAILURE_DETAIL}; worker log: {}",
        paths::task_dir(task_id).join("worker.log").display()
    );
    if record_failure(store, task_id, &detail, &detail)? {
        cleaned.push(task_id.to_string());
    }
    super::clear_spec(task_id)?;
    crate::runtime::execution_close::close_warn(
        store,
        task_id,
        "lost",
        crate::runtime::execution_close::Closer::Reaper,
        Some(&evidence),
    );
    Ok(())
}

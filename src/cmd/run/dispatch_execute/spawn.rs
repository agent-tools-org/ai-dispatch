// Worker spawn boundary distinguishes pre-spawn failure from later publication errors.
// The transient marker classifies errors only; receipts continue to read persisted execution facts.
use super::*;

pub(super) fn spawn_background_worker(
    store: &Store, spec: &BackgroundRunSpec, caller_owned: bool,
) -> Result<u32> {
    let spawned = std::cell::Cell::new(None);
    let result = spawn_inner(store, spec, &spawned, caller_owned);
    if caller_owned && let Some(pid) = spawned.get() {
        if let Err(error) = store.observe_worker_state(&spec.task_id, "running", Some(pid), None) {
            aid_warn!("[aid] Could not record worker PID {}: {error}", spec.task_id);
        }
    }
    result.map_err(|error| {
        if !caller_owned {
            return error.downcast::<WorkerSpawned>().map_or_else(|error| error, |spawned| spawned.0);
        }
        if spawned.get().is_some() || error.is::<WorkerSpawned>() {
            if let Err(persist) = store.mark_worker_spawned(&spec.task_id) {
                aid_warn!("[aid] Could not record worker spawn {}: {persist}", spec.task_id);
            }
            WorkerSpawned(error).into()
        } else {
            error
        }
    })
}

fn spawn_inner(
    store: &Store, spec: &BackgroundRunSpec, spawned: &std::cell::Cell<Option<u32>>, caller_owned: bool,
) -> Result<u32> {
    background::save_spec(spec)?;
    #[cfg(not(test))]
    let spawn = background::spawn_worker(spec.task_id.as_str())?;
    let worker_pid = store.with_worker_launch(caller_owned, spec.task_id.as_str(), || {
        #[cfg(test)]
        let pid = tests::spawn_worker()?;
        #[cfg(not(test))]
        let (launcher, pid) = spawn()?;
        spawned.set(Some(pid));
        let published = background::update_worker_pid(spec.task_id.as_str(), pid);
        if published.is_err() {
            background::sigkill_process(pid);
        }
        #[cfg(test)]
        {
            Ok((pid, published))
        }
        #[cfg(not(test))]
        {
            Ok((launcher, pid, published))
        }
    })?;
    #[cfg(test)]
    let (worker_pid, published) = worker_pid;
    #[cfg(not(test))]
    let (launcher, worker_pid, published) = worker_pid;
    #[cfg(not(test))]
    acknowledge(launcher, worker_pid, published.is_ok())?;
    published?;
    Ok(worker_pid)
}

#[cfg(not(test))]
fn acknowledge(mut launcher: std::process::Child, worker_pid: u32, published: bool) -> Result<()> {
    use std::io::Write;
    let acknowledged = launcher
        .stdin
        .take()
        .filter(|_| published)
        .map(|mut pipe| pipe.write_all(b"1"));
    std::thread::spawn(move || {
        let _ = launcher.wait();
    });
    if let Some(Err(error)) = acknowledged {
        background::sigkill_process(worker_pid);
        return Err(error.into());
    }
    Ok(())
}

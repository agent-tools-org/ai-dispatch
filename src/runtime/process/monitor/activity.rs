// Shared descendant activity probe for stream, PTY and background watchdogs.
// Persists CPU samples so separate reaper processes observe growth, not mere existence.
// Deps: Unix ps, Linux procfs, serde, private task paths and std time/filesystem.

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

#[derive(Clone, Serialize, Deserialize)]
struct Process {
    pid: u32,
    ppid: u32,
    group: u32,
    started: String,
    cpu: u64,
    remote_wait: bool,
}

#[derive(Serialize, Deserialize)]
struct Sample {
    root: u32,
    started: String,
    sampled_at: SystemTime,
    progress_at: SystemTime,
    children: Vec<Process>,
}

pub(crate) fn latest(task_id: &str, root: Option<u32>, window: Duration) -> Option<SystemTime> {
    let root = root?;
    let started = root_started(root)?;
    latest_owned(task_id, Some(root), Some(&started), window)
}

pub(crate) fn latest_owned(
    task_id: &str, root: Option<u32>, started: Option<&str>, window: Duration,
) -> Option<SystemTime> {
    let root = root.filter(|pid| *pid > 1)?;
    let started = started?;
    if !root_matches(root, Some(started)) {
        return None;
    }
    let path = crate::support::paths::task_dir(task_id).join("child-activity.json");
    let previous = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Sample>(&bytes).ok())
        .filter(|sample| sample.root == root && sample.started == started);
    let now = SystemTime::now();
    let interval = (window / 4).min(Duration::from_secs(1));
    if let Some(sample) = previous.as_ref()
        && now.duration_since(sample.sampled_at).ok()? < interval
    {
        return Some(sample.progress_at);
    }
    let table = process_table()?;
    let leader = table.iter().find(|process| process.pid == root)?;
    if leader.started != started {
        return None;
    }
    let children = descendants(root, &table);
    let previous = previous.filter(|sample| sample.started == leader.started);
    if children.is_empty() {
        return previous.map(|sample| sample.progress_at);
    }
    let progress_at = advance(previous.as_ref(), &children, leader.group, now);
    let sample = Sample {
        root,
        started: leader.started.clone(),
        sampled_at: now,
        progress_at,
        children,
    };
    persist(&path, &sample)?;
    Some(progress_at)
}

pub(crate) fn root_matches(pid: u32, started: Option<&str>) -> bool {
    started.is_some_and(|started| root_started(pid).as_deref() == Some(started))
}

pub(crate) fn root_started(pid: u32) -> Option<String> {
    if pid <= 1 { return None; }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        return Some(proc_cpu(&stat)?.1.to_string());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = identity_command(pid)
            .output().ok().filter(|output| output.status.success())?;
        let started = String::from_utf8(output.stdout).ok()?
            .split_whitespace().collect::<Vec<_>>().join(" ");
        (!started.is_empty()).then_some(started)
    }
}

#[cfg(any(not(target_os = "linux"), test))]
fn identity_command(pid: u32) -> std::process::Command {
    let mut command = ps_command();
    command.args(["-p", &pid.to_string(), "-o", "lstart="]);
    command
}

fn advance(
    previous: Option<&Sample>,
    children: &[Process],
    group: u32,
    now: SystemTime,
) -> SystemTime {
    let Some(previous) = previous else {
        return now;
    };
    let progressing = children.iter().any(|child| {
        if child.remote_wait && child.group == group {
            return true;
        }
        let old = previous
            .children
            .iter()
            .find(|old| old.pid == child.pid && old.started == child.started);
        child.cpu > old.map_or(0, |old| old.cpu)
    });
    if progressing {
        now
    } else {
        previous.progress_at
    }
}

fn persist(path: &std::path::Path, sample: &Sample) -> Option<()> {
    use std::io::Write;
    let parent = path.parent()?;
    let bytes = serde_json::to_vec(sample).ok()?;
    std::fs::create_dir_all(parent).ok()?;
    let temporary = path.with_extension(format!("{:032x}.tmp", rand::random::<u128>()));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(&bytes));
    let result = written.and_then(|()| std::fs::rename(&temporary, path));
    let _ = std::fs::remove_file(temporary);
    result.ok()
}

pub(crate) fn ps_command() -> std::process::Command {
    let mut command = std::process::Command::new("ps");
    command.env("TZ", "UTC0").env("LC_ALL", "C");
    command
}

fn table_command() -> std::process::Command {
    let mut command = ps_command();
    command.args([
        "-A", "-o", "pid=", "-o", "ppid=", "-o", "pgid=", "-o", "time=", "-o", "lstart=", "-o",
        "stat=", "-o", "args=",
    ]);
    command
}

fn process_table() -> Option<Vec<Process>> {
    let output = table_command().output().ok().filter(|output| output.status.success())?;
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_process)
            .collect(),
    )
}

fn parse_process(line: &str) -> Option<Process> {
    let mut fields = line.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let ppid = fields.next()?.parse().ok()?;
    let group = fields.next()?.parse().ok()?;
    let cpu = cpu_time(fields.next()?)?;
    let started = fields.by_ref().take(5).collect::<Vec<_>>().join(" ");
    let state = fields.next()?;
    if state.starts_with('Z') {
        return None;
    }
    let remote_wait = !state.starts_with('T') && is_remote_wait(&fields.collect::<Vec<_>>());
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut process = Process {
        pid,
        ppid,
        group,
        cpu,
        started,
        remote_wait,
    };
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (cpu, started) = proc_cpu(&stat)?;
        process.cpu = cpu;
        process.started = started.to_string();
    }
    Some(process)
}

fn cpu_time(time: &str) -> Option<u64> {
    let (days, time) = match time.split_once('-') {
        Some((days, time)) => (days.parse::<u64>().ok()?, time),
        None => (0, time),
    };
    let mut total = 0f64;
    for field in time.split(':') {
        total = total * 60.0 + field.parse::<f64>().ok()?;
    }
    Some(((total + days as f64 * 86400.0) * 1_000_000.0) as u64)
}

fn proc_cpu(stat: &str) -> Option<(u64, &str)> {
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields = fields.split_whitespace().collect::<Vec<_>>();
    let cpu = fields
        .get(11)?
        .parse::<u64>()
        .ok()?
        .checked_add(fields.get(12)?.parse::<u64>().ok()?)?;
    Some((cpu, fields.get(19)?))
}

fn is_remote_wait(args: &[&str]) -> bool {
    fn basename(value: &str) -> &str {
        value.rsplit('/').next().unwrap_or(value)
    }
    let Some(program) = args.first().map(|arg| basename(arg)) else {
        return false;
    };
    let args = if matches!(program, "sh" | "bash") || program.starts_with("python") {
        &args[1..]
    } else {
        args
    };
    match args.first().map(|arg| basename(arg)) {
        Some("ssh") => true,
        Some("rbox") => args.get(1).is_some_and(|verb| {
            matches!(
                *verb,
                "exec" | "run" | "sync" | "resume" | "ensure" | "wait"
            ) || (*verb == "task" && args.contains(&"--wait"))
        }),
        _ => false,
    }
}

fn descendants(root: u32, table: &[Process]) -> Vec<Process> {
    let mut found = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for process in table {
            if process.ppid == parent
                && process.pid != root
                && !found.iter().any(|seen: &Process| seen.pid == process.pid)
            {
                frontier.push(process.pid);
                found.push(process.clone());
            }
        }
    }
    found
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "activity_identity_tests.rs"]
mod identity_tests;

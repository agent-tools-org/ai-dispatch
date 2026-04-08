// E2E coverage for batch slot refills after background task completion.
// Verifies pending work starts promptly when --max-concurrent slots free up.
// Deps: compiled `aid` binary, tempfile, and a custom shell-backed agent.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

fn aid_cmd_in(aid_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd.env("AID_NO_DETACH", "1");
    cmd
}

#[test]
fn batch_refills_pending_tasks_when_slots_free_up() {
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(
        script_dir.path(),
        "fast-batch-agent",
        "#!/bin/sh\nsleep 0.2\nprintf 'done\\n'\n",
    );
    write_custom_agent(aid_home.path(), "fastbatch", &agent_path);

    let batch_file = aid_home.path().join("tasks.toml");
    std::fs::write(&batch_file, batch_file_contents("fastbatch", 8)).unwrap();

    let started_at = Instant::now();
    let output = aid_cmd_in(aid_home.path())
        .args([
            "batch",
            batch_file.to_str().unwrap(),
            "--parallel",
            "--max-concurrent",
            "4",
        ])
        .output()
        .unwrap();
    let elapsed = started_at.elapsed();

    assert!(
        output.status.success(),
        "batch failed after {elapsed:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "pending tasks did not refill promptly: batch took {elapsed:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn quiet_batch_emits_progress_before_completion() {
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(
        script_dir.path(),
        "slow-batch-agent",
        "#!/bin/sh\nsleep 4\nprintf 'done\\n'\n",
    );
    write_custom_agent(aid_home.path(), "slowbatch", &agent_path);

    let batch_file = aid_home.path().join("tasks.toml");
    std::fs::write(&batch_file, batch_file_contents("slowbatch", 1)).unwrap();

    let mut child = aid_cmd_in(aid_home.path())
        .args(["batch", batch_file.to_str().unwrap(), "--quiet"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let stdout = std::io::BufReader::new(stdout);
        for line in std::io::BufRead::lines(stdout) {
            let line = line.unwrap();
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_millis(2500);
    let mut saw_progress = false;
    let mut saw_while_running = false;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = rx.recv_timeout(remaining) else {
            break;
        };
        if line.contains("[batch]") && line.contains("dispatched") {
            saw_progress = true;
            saw_while_running = child.try_wait().unwrap().is_none();
            break;
        }
    }

    let status = child.wait().unwrap();
    reader.join().unwrap();

    assert!(status.success());
    assert!(saw_progress, "quiet batch did not emit progress before completion");
    assert!(saw_while_running, "quiet batch progress was not visible until the process had already exited");
}

fn batch_file_contents(agent: &str, task_count: usize) -> String {
    let mut batch = String::new();
    for index in 0..task_count {
        batch.push_str(&format!(
            "[[tasks]]\nname = \"task-{index}\"\nagent = \"{agent}\"\nprompt = \"task {index}\"\n\n"
        ));
    }
    batch
}

fn write_custom_agent(aid_home: &Path, id: &str, command: &Path) {
    let agents_dir = aid_home.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{id}.toml")),
        format!(
            "[agent]\nid = \"{id}\"\ndisplay_name = \"{id}\"\ncommand = \"{}\"\ntrust_tier = \"local\"\n",
            command.display()
        ),
    )
    .unwrap();
}

fn write_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    #[cfg(unix)]
    {
        let permissions = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    path
}

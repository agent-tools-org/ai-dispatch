// CLI batch flows for dependency skips, completion summaries and retry selection.
// Retains existing E2E assertions with controlled agent executables.
use super::aid_cmd_in;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[cfg(unix)]
#[test]
fn board_shows_skipped_batch_task_when_dependency_fails() {
    let temp_dir = TempDir::new().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    write_fake_codex(&bin_dir, "#!/bin/sh\nexit 1\n");

    let batch_path = write_batch(&temp_dir, DEPENDENT_TASKS);

    let path = std::env::var("PATH").unwrap_or_default();
    let test_path = format!("{}:{}", bin_dir.display(), path);

    let batch_output = aid_cmd_in(temp_dir.path())
        .env("PATH", &test_path)
        .args(["batch", batch_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(batch_output.status.success());

    let stderr = String::from_utf8_lossy(&batch_output.stderr);
    let skip_line = stderr
        .lines()
        .find(|line| line.contains("[batch] Skipping task B ("))
        .unwrap();
    let skipped_task_id = skip_line
        .split('(')
        .nth(1)
        .and_then(|part| part.split(')').next())
        .unwrap();

    let board_output = aid_cmd_in(temp_dir.path())
        .args(["board"])
        .output()
        .unwrap();
    assert!(board_output.status.success());

    let stdout = String::from_utf8_lossy(&board_output.stdout);
    assert!(stdout.contains(skipped_task_id));
    assert!(stdout.contains("SKIP"));
}

#[cfg(unix)]
#[test]
fn batch_completion_summary_prints_stats() {
    let temp_dir = TempDir::new().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    write_fake_codex(&bin_dir, "#!/bin/sh\nexit 0\n");

    let batch_path = write_batch(&temp_dir, TWO_TASKS);

    let path = std::env::var("PATH").unwrap_or_default();
    let test_path = format!("{}:{}", bin_dir.display(), path);

    let output = aid_cmd_in(temp_dir.path())
        .env("PATH", &test_path)
        .args(["batch", batch_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("done"));
    assert!(stderr.contains("failed"));
    assert!(stderr.contains("skipped"));
    assert!(stderr.contains("Cost:") || stderr.contains("Time:"));
}

#[cfg(unix)]
#[test]
fn batch_retry_with_no_failures_succeeds() {
    let temp_dir = TempDir::new().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    write_fake_codex(&bin_dir, RETRY_CODEX);

    let batch_path = write_batch(&temp_dir, TWO_TASKS);

    let path = std::env::var("PATH").unwrap_or_default();
    let test_path = format!("{}:{}", bin_dir.display(), path);

    let batch_output = aid_cmd_in(temp_dir.path())
        .env("PATH", &test_path)
        .args(["batch", batch_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(batch_output.status.success());

    let batch_stderr = String::from_utf8_lossy(&batch_output.stderr);
    let workgroup_id = batch_stderr
        .lines()
        .find_map(|line| {
            let marker = "Auto-created workgroup ";
            let (_, rest) = line.split_once(marker)?;
            rest.split_whitespace().next().map(str::to_string)
        })
        .unwrap();

    let retry_output = aid_cmd_in(temp_dir.path())
        .env("PATH", &test_path)
        .args(["batch", "retry", &workgroup_id])
        .output()
        .unwrap();
    assert!(
        retry_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&retry_output.stdout),
        String::from_utf8_lossy(&retry_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&retry_output.stdout);
    let stderr = String::from_utf8_lossy(&retry_output.stderr);
    assert!(stdout.contains("No retryable tasks") || stderr.contains("No retryable tasks"));
}

#[cfg(unix)]
fn write_fake_codex(bin_dir: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir(bin_dir).unwrap();
    let path = bin_dir.join("codex");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn write_batch(home: &TempDir, tasks: &str) -> PathBuf {
    let path = home.path().join("batch.toml");
    std::fs::write(&path, tasks).unwrap();
    path
}

#[cfg(unix)]
const RETRY_CODEX: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.147.0"
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  echo "      --ignore-user-config"
  exit 0
fi
printf '%s\n' '{"type":"thread.started","thread_id":"batch-retry-fixture"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"ok"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
"#;

const DEPENDENT_TASKS: &str = concat!(
    "[[task]]\n",
    "name = \"A\"\n",
    "agent = \"codex\"\n",
    "prompt = \"task A\"\n",
    "\n",
    "[[task]]\n",
    "name = \"B\"\n",
    "agent = \"codex\"\n",
    "prompt = \"task B\"\n",
    "depends_on = [\"A\"]\n",
);

const TWO_TASKS: &str = concat!(
    "[[task]]\n",
    "name = \"A\"\n",
    "agent = \"codex\"\n",
    "prompt = \"task A\"\n",
    "\n",
    "[[task]]\n",
    "name = \"B\"\n",
    "agent = \"codex\"\n",
    "prompt = \"task B\"\n",
);

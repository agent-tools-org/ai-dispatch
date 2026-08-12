// E2E proof that the shared harness isolates project discovery from the ambient cwd.
// Observes the inherited verify command under a project-bearing cwd, then none under isolation.
// Deps: compiled `aid` binary, tempfile, rusqlite, a shell-backed custom agent.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

mod common;
use common::{aid_cmd_in, aid_cmd_with_cwd};

/// Fingerprint verify command that is cheap and unmistakable in the task row.
const FINGERPRINT_VERIFY: &str = "echo FINGERPRINT_VERIFY_OK";

#[test]
fn harness_isolates_project_verify_from_ambient_cwd() {
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(
        script_dir.path(),
        "iso-agent",
        "#!/bin/sh\nprintf 'done\\n'\n",
    );

    // --- LEAK side: cwd is a temp git repo with a distinctive project verify ---
    let leak_home = TempDir::new().unwrap();
    write_custom_agent(leak_home.path(), "isoagent", &agent_path);
    let project_repo = TempDir::new().unwrap();
    init_git_repo(project_repo.path());
    write_project_verify(project_repo.path(), FINGERPRINT_VERIFY);

    run_ok(
        aid_cmd_with_cwd(leak_home.path(), project_repo.path()).args([
            "run",
            "isoagent",
            "probe inherited verify",
            "--id",
            "t-leak-verify",
        ]),
    );
    let leaked = task_verify(leak_home.path(), "t-leak-verify");
    assert_eq!(
        leaked.as_deref(),
        Some(FINGERPRINT_VERIFY),
        "without isolation, project discovery must stamp the task's verify column"
    );

    // --- ISOLATED side: default harness points cwd at AID_HOME (no git root) ---
    let iso_home = TempDir::new().unwrap();
    write_custom_agent(iso_home.path(), "isoagent", &agent_path);

    run_ok(aid_cmd_in(iso_home.path()).args([
        "run",
        "isoagent",
        "probe isolated verify",
        "--id",
        "t-iso-verify",
    ]));
    let isolated = task_verify(iso_home.path(), "t-iso-verify");
    assert_eq!(
        isolated, None,
        "aid_cmd_in must not inherit any ambient project verify; got {isolated:?}"
    );
}

fn task_verify(aid_home: &Path, task_id: &str) -> Option<String> {
    let conn = Connection::open(aid_home.join("aid.db")).ok()?;
    conn.query_row(
        "SELECT verify FROM tasks WHERE id = ?1",
        [task_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()?
}

fn run_ok(cmd: &mut Command) -> std::process::Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn init_git_repo(dir: &Path) {
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(dir)
        .status()
        .expect("git init");
    assert!(status.success(), "git init failed in {}", dir.display());
}

fn write_project_verify(repo: &Path, verify: &str) {
    let aid_dir = repo.join(".aid");
    std::fs::create_dir_all(&aid_dir).unwrap();
    std::fs::write(
        aid_dir.join("project.toml"),
        format!("[project]\nid = \"e2e-probe\"\nverify = \"{verify}\"\n"),
    )
    .unwrap();
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

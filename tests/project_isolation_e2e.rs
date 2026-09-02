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

struct CrossProjectFixture {
    _root: TempDir,
    aid_home: TempDir,
    caller: PathBuf,
    target: PathBuf,
    agent_target_capture: PathBuf,
    verify_target_capture: PathBuf,
    target_verify: String,
}

impl CrossProjectFixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let aid_home = TempDir::new().unwrap();
        let caller = root.path().join("caller");
        let target = root.path().join("target");
        std::fs::create_dir_all(&caller).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        init_git_repo(&caller);
        init_git_repo(&target);
        let agent_target_capture = root.path().join("agent-cargo-target.txt");
        let verify_target_capture = root.path().join("verify-cargo-target.txt");
        let agent = write_script(
            root.path(), "route-agent",
            "#!/bin/sh\nprintf '%s\\n' \"$CARGO_TARGET_DIR\" > \"$TARGET_CAPTURE\"\nprintf 'done\\n'\n",
        );
        let verifier = write_script(
            root.path(), "route-verify",
            "#!/bin/sh\nprintf '%s\\n' \"$CARGO_TARGET_DIR\" > \"$VERIFY_TARGET_CAPTURE\"\n",
        );
        let target_verify = format!("sh {}", verifier.display());
        write_project(&caller, "caller-project", FINGERPRINT_VERIFY);
        write_project(&target, "target-project", &target_verify);
        std::fs::write(
            target.join("Cargo.toml"),
            "[package]\nname = \"target\"\nversion = \"0.1.0\"\n",
        ).unwrap();
        write_custom_agent(aid_home.path(), "routeagent", &agent);
        Self {
            _root: root, aid_home, caller, target, agent_target_capture,
            verify_target_capture, target_verify,
        }
    }
}

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
    write_project(project_repo.path(), "e2e-probe", FINGERPRINT_VERIFY);

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

#[test]
fn explicit_dir_uses_the_target_projects_verify_identity_and_cargo_cache() {
    let fixture = CrossProjectFixture::new();
    let caller_target = fixture
        ._root
        .path()
        .join("cargo-target/caller-project/caller-branch");

    run_ok(
        aid_cmd_with_cwd(fixture.aid_home.path(), &fixture.caller)
            .env("CARGO_TARGET_DIR", &caller_target)
            .env("TARGET_CAPTURE", &fixture.agent_target_capture)
            .env("VERIFY_TARGET_CAPTURE", &fixture.verify_target_capture)
            .args([
                "run",
                "routeagent",
                "Implement the target repository change.",
                "--dir",
                fixture.target.to_str().unwrap(),
                "--id",
                "t-cross-project-route",
            ]),
    );

    let (verify, project_id) = task_route(fixture.aid_home.path(), "t-cross-project-route");
    assert_eq!(verify.as_deref(), Some(fixture.target_verify.as_str()));
    assert_eq!(project_id.as_deref(), Some("target-project"));
    let expected_target = fixture
        ._root
        .path()
        .join("cargo-target/target-project/_base");
    assert_captured_target(&fixture.agent_target_capture, &expected_target);
    assert_captured_target(&fixture.verify_target_capture, &expected_target);
}

fn assert_captured_target(capture: &Path, expected: &Path) {
    assert_eq!(
        std::fs::read_to_string(capture).unwrap().trim(),
        expected.to_string_lossy()
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

fn task_route(aid_home: &Path, task_id: &str) -> (Option<String>, Option<String>) {
    let conn = Connection::open(aid_home.join("aid.db")).unwrap();
    conn.query_row(
        "SELECT verify, project_id FROM tasks WHERE id = ?1",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
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

fn write_project(repo: &Path, id: &str, verify: &str) {
    let aid_dir = repo.join(".aid");
    std::fs::create_dir_all(&aid_dir).unwrap();
    std::fs::write(
        aid_dir.join("project.toml"),
        format!("[project]\nid = \"{id}\"\nverify = \"{verify}\"\n"),
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

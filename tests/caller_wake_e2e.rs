// E2E: a settled --bg task is pushed into the dispatching session's inbox socket,
// the agent never inherits that socket, and the unread notice clears once the caller reads it.
// Deps: compiled aid binary, tests/common, a Unix listener standing in for Claude Code.
#![cfg(unix)]

#[allow(dead_code)]
mod common;

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::TempDir;

const SESSION: &str = "e2e-caller-session";
const TOKEN: &str = "e2e-token";

fn write_agent(home: &Path, scripts: &Path) {
    let dump = scripts.join("agent-env");
    let script = scripts.join("wake-agent");
    common::write_executable(
        &script,
        &format!(
            "#!/bin/sh\nenv | grep -c '^CLAUDE_CODE_MESSAGING' > '{}'\n\
             printf '%s\\n' '{{\"type\":\"completion\",\"finalText\":\"done\"}}'\n",
            dump.display()
        ),
    );
    let agents = home.join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("wake.toml"),
        format!(
            "[agent]\nid = \"wake\"\ndisplay_name = \"wake\"\ncommand = \"{}\"\n\
             trust_tier = \"local\"\nstreaming = true\noutput_format = \"jsonl\"\n",
            script.display()
        ),
    )
    .unwrap();
}

fn aid(home: &Path, project: &Path, socket: &Path) -> Command {
    let mut cmd = common::aid_cmd_with_cwd(home, project);
    cmd.env("CLAUDE_CODE_SESSION_ID", SESSION)
        .env("CLAUDE_CODE_MESSAGING_SOCKET", socket)
        .env("CLAUDE_CODE_MESSAGING_TOKEN", TOKEN);
    cmd
}

fn hook_text(cmd: &mut Command) -> String {
    let output = cmd.args(["hook", "session-start"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn settled_bg_task_wakes_its_caller_and_agent_never_sees_the_socket() {
    let (home, project, scripts) = (TempDir::new().unwrap(), TempDir::new().unwrap(), TempDir::new().unwrap());
    write_agent(home.path(), scripts.path());
    // A short path: Unix socket paths are limited to about 104 bytes.
    let sock_dir = tempfile::Builder::new().prefix("cw").tempdir_in("/tmp").unwrap();
    let socket = sock_dir.path().join("s");
    let listener = UnixListener::bind(&socket).unwrap();
    let (sent, inbox) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut text = String::new();
            let _ = stream.read_to_string(&mut text);
            let _ = sent.send(text);
        }
    });

    let id = "t-callerwake";
    let output = aid(home.path(), project.path(), &socket)
        .args(["run", "wake", "wake the caller", "--id", id, "--bg", "--dir"])
        .arg(project.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let text = inbox.recv_timeout(Duration::from_secs(90)).expect("caller was never pushed");
    let mut lines = text.lines();
    let auth: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(auth["token"], TOKEN);
    let user: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    let content = user["message"]["content"].as_str().unwrap();
    assert!(content.contains(id) && content.contains("aid show"), "{content}");
    assert!(!content.contains("wake the caller"), "prompt leaked: {content}");
    let leaked = std::fs::read_to_string(scripts.path().join("agent-env")).unwrap();
    assert_eq!(leaked.trim(), "0", "agent inherited the caller socket");

    // Pushed is not read: a resumed session still lists it until the caller reads it.
    assert!(hook_text(&mut aid(home.path(), project.path(), &socket)).contains(id));
    let show = aid(home.path(), project.path(), &socket).args(["show", id]).output().unwrap();
    assert!(show.status.success(), "{}", String::from_utf8_lossy(&show.stderr));
    assert!(!hook_text(&mut aid(home.path(), project.path(), &socket)).contains(id));
}

// Opt-in native check that the emitted Codex permission profile governs loopback network.
// Runs `codex sandbox` with aid's profile against a local HTTP listener, with and without --network.
// Deps: installed codex and curl, OS sandbox, parent command builder.

use super::CodexAgent;
use crate::agent::{Agent, CommandContext, RunOpts};
use std::{io::{Read, Write}, net::TcpListener, process::Command};

fn serve_loopback() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

fn sandbox_status(dir: &std::path::Path, home: &std::path::Path, network: bool, port: u16) -> String {
    let opts = RunOpts { dir: Some(dir.display().to_string()), network, ..RunOpts::default() };
    let context = CommandContext { durable_codex_home: false, writable_roots: vec![] };
    let built = CodexAgent.build_command_with_context("probe", &opts, context).unwrap();
    let args: Vec<String> = crate::support::test_command::args(&built);
    let permissions = args.iter().find(|arg| arg.starts_with("permissions=")).unwrap();
    let output = Command::new("codex")
        .env("CODEX_HOME", home)
        .args(["sandbox", "-C"]).arg(dir)
        .args(["-c", permissions, "-P", "aid-write", "--"])
        .args(["curl", "-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "5"])
        .arg(format!("http://127.0.0.1:{port}/"))
        .output()
        .unwrap();
    eprintln!("network={network}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
#[ignore = "requires native Codex CLI, curl and OS sandbox on the build box"]
fn native_codex_profile_blocks_loopback_unless_network_is_granted() {
    let temp = tempfile::tempdir().unwrap();
    let (dir, home) = (temp.path().join("work"), temp.path().join("codex-home"));
    std::fs::create_dir(&dir).unwrap();
    std::fs::create_dir(&home).unwrap();
    let port = serve_loopback();
    assert_eq!(sandbox_status(&dir, &home, true, port), "200");
    assert_ne!(sandbox_status(&dir, &home, false, port), "200");
}

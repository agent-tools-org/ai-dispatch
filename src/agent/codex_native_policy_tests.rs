// Opt-in native CLI policy regression with a hermetic mock Responses endpoint.
// Checks read-only/write transitions across fresh and resumed Codex sessions.
// Deps: installed codex, python3 stdlib fixture and parent command builder.

use super::CodexAgent;
use crate::agent::{Agent, CommandContext, RunOpts};
use std::{fs, process::Command};

#[test]
#[ignore = "requires native Codex CLI and OS sandbox on the build box"]
fn fix_round_one_native_codex_fresh_and_resumed_permission_transitions() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let scratch = temp.path().join("scratch");
    let home = temp.path().join("codex-home");
    for dir in [&repo, &scratch, &home] {
        fs::create_dir(dir).unwrap();
    }
    assert!(
        Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::create_dir(repo.join(".codex")).unwrap();
    fs::write(
        repo.join(".codex/config.toml"),
        "sandbox_mode = 'danger-full-access'\n",
    )
    .unwrap();
    let invocations = invocations(&repo, &scratch);
    let script = temp.path().join("native-probe.py");
    fs::write(&script, include_str!("codex_native_policy_fixture.py")).unwrap();
    let output = Command::new("python3")
        .arg(&script)
        .arg(&repo)
        .arg(&scratch)
        .arg(&home)
        .arg(serde_json::to_string(&invocations).unwrap())
        .output()
        .unwrap();
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn invocations(repo: &std::path::Path, scratch: &std::path::Path) -> Vec<serde_json::Value> {
    let mut invocations = Vec::new();
    for (read_only, session) in [(true, None), (false, Some("SESSION")), (true, Some("SESSION"))] {
        let opts = RunOpts {
            dir: Some(repo.display().to_string()),
            read_only,
            session_id: session.map(str::to_string),
            model: Some("gpt-4.1".into()),
            ..super::command_tests::test_opts()
        };
        let cmd = CodexAgent
            .build_command_with_context(
                "native policy probe",
                &opts,
                CommandContext {
                    durable_codex_home: false,
                    writable_roots: vec![scratch.to_path_buf()],
                },
            )
            .unwrap();
        invocations.push(serde_json::json!({
            "read_only": read_only,
            "argv": cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>()
        }));
    }
    invocations
}

// Cursor identity regressions using literal vendor help and executable PATH fixtures.
// Exercises shared availability checks and dispatch resolution in isolated subprocesses.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const GROK_HELP: &str = "cursor-worker  Register this machine as a Cursor private worker";
const CURSOR_HELP: &str = "Usage: agent [options] [command] [prompt...]\n\nStart the Cursor Agent";

#[test]
fn literal_vendor_help_identifies_only_cursor() {
    let _permit = crate::test_subprocess::acquire();
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("agent");
    let marker = super::super::env_identity::identity_marker("agent").unwrap();
    for (help, expected) in [(GROK_HELP, false), (CURSOR_HELP, true), ("", false)] {
        write_agent(&binary, help);
        let path = binary.to_str().unwrap();
        assert_eq!(super::identifies_as_cursor(path), expected, "{help}");
        assert_eq!(
            super::super::env_identity::binary_identity_matches(path, marker),
            expected,
            "{help}"
        );
    }
}

#[test]
fn cursor_agent_wins_after_grok_agent_on_path() {
    let _permit = crate::test_subprocess::acquire();
    let root = tempfile::tempdir().unwrap();
    let grok = root.path().join("grok");
    let cursor = root.path().join("cursor");
    fs::create_dir_all(&grok).unwrap();
    fs::create_dir_all(&cursor).unwrap();
    write_agent(&grok.join("agent"), GROK_HELP);
    write_agent(&cursor.join("agent"), CURSOR_HELP);
    let output = probe_path(&[&grok, &cursor]);
    assert!(output.lines().any(|line| line == "CURSOR_AVAILABLE=true"));
    let expected = format!("CURSOR_BINARY={}", cursor.join("agent").display());
    assert!(output.lines().any(|line| line == expected), "{output}");
}

#[test]
fn grok_cursor_worker_does_not_make_cursor_available() {
    let _permit = crate::test_subprocess::acquire();
    let root = tempfile::tempdir().unwrap();
    write_agent(&root.path().join("agent"), GROK_HELP);
    let output = probe_path(&[root.path()]);
    assert!(output.lines().any(|line| line == "CURSOR_AVAILABLE=false"));
    assert!(output.lines().any(|line| line == "CURSOR_BINARY=cursor-agent"));
}

#[test]
#[ignore]
fn reports_cursor_identity_for_subprocess() {
    println!("CURSOR_AVAILABLE={}", super::super::env::which_exists("agent"));
    println!("CURSOR_BINARY={}", super::cursor_binary());
}

fn write_agent(path: &Path, help: &str) {
    fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' '{help}'\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn probe_path(dirs: &[&Path]) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "agent::cursor::identity_tests::reports_cursor_identity_for_subprocess",
            "--ignored",
            "--nocapture",
        ])
        .env("PATH", std::env::join_paths(dirs).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn resolve_cursor_binary_walks_path_with_stubbed_identity() {
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn write_agent(dir: &Path, executable: bool) {
        fs::create_dir_all(dir).unwrap();
        let agent = dir.join("agent");
        fs::write(&agent, "#!/bin/sh\nexit 0\n").unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        let mut perms = fs::metadata(&agent).unwrap().permissions();
        perms.set_mode(mode);
        fs::set_permissions(&agent, perms).unwrap();
    }

    let root = tempfile::tempdir().unwrap();
    let decoy = root.path().join("decoy");
    let cursor_a = root.path().join("cursor_a");
    let cursor_b = root.path().join("cursor_b");
    let nonexec = root.path().join("nonexec");
    write_agent(&decoy, true);
    write_agent(&cursor_a, true);
    write_agent(&cursor_b, true);
    write_agent(&nonexec, false);

    let decoy_agent = decoy.join("agent").to_str().unwrap().to_owned();
    let cursor_a_agent = cursor_a.join("agent").to_str().unwrap().to_owned();
    let cursor_b_agent = cursor_b.join("agent").to_str().unwrap().to_owned();
    let nonexec_agent = nonexec.join("agent").to_str().unwrap().to_owned();

    // Literal expected values (not re-derived from the function under test).
    let expected_decoy_then_cursor = cursor_b_agent.clone();
    let expected_cursor_first = cursor_a_agent.clone();
    let expected_no_cursor = "cursor-agent".to_owned();
    let expected_skip_nonexec = cursor_a_agent.clone();

    let cases = [
        (
            "decoy_then_cursor",
            vec![decoy.clone(), cursor_b.clone()],
            expected_decoy_then_cursor,
        ),
        (
            "cursor_first",
            vec![cursor_a.clone(), decoy.clone()],
            expected_cursor_first,
        ),
        ("no_cursor", vec![decoy.clone()], expected_no_cursor),
        (
            "skip_nonexec",
            vec![nonexec.clone(), cursor_a.clone()],
            expected_skip_nonexec,
        ),
    ];

    for (name, dirs, expected) in cases {
        let path = std::env::join_paths(dirs.iter()).unwrap();
        let probed = RefCell::new(Vec::<String>::new());
        let cursor_a_hit = cursor_a_agent.clone();
        let cursor_b_hit = cursor_b_agent.clone();
        let got = super::resolve_cursor_binary_from_path(Some(path), |candidate| {
            probed.borrow_mut().push(candidate.to_owned());
            candidate == cursor_a_hit || candidate == cursor_b_hit
        });
        assert_eq!(got, expected.as_str(), "case {name}");
        assert!(
            !probed.borrow().iter().any(|p| p == &nonexec_agent),
            "case {name}: non-executable agent must not be probed"
        );
        if name == "decoy_then_cursor" {
            assert_eq!(
                probed.borrow().as_slice(),
                &[decoy_agent.as_str(), cursor_b_agent.as_str()],
                "case {name}: probe order"
            );
        }
    }
}

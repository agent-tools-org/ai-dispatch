// Tests for recognising Cursor's `agent` binary among same-named executables.
// Deps: env_identity markers, temp PATH directories with stub executables.

use std::fs;

#[test]
fn only_accepts_a_bare_agent_binary_that_identifies_as_cursor() {
    // Cursor's own help text names the product; xAI's Grok Build CLI ships a binary with
    // the same `agent` name, and its help mentions a `cursor-worker` subcommand.
    let markers = crate::agent::env_identity::identity_markers("agent").unwrap();
    let identifies = |help| crate::agent::env_identity::help_identifies(help, markers);
    assert!(identifies("Usage: cursor-agent [OPTIONS]\n  -p, --print  Print response\n"));
    assert!(identifies("Start the Cursor Agent\n\nUsage: agent [options]\n"));
    assert!(!identifies("Grok Build TUI\n\nUsage: agent [OPTIONS] [PROMPT] [COMMAND]\n"));
    assert!(!identifies(
        "Usage: agent [OPTIONS] [PROMPT] [COMMAND]\n  cursor-worker  Register this machine as a \
         Cursor private worker (via the leader)\n"
    ));
    assert!(!identifies(""));
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

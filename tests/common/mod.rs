// Shared integration-test command helpers for invoking the `aid` binary.
// Exports helpers that isolate subprocess state with a temp AID_HOME.
// Deps: std::process::Command and temp directories supplied by callers.

use std::path::Path;
use std::process::Command;

/// Build an `aid` subprocess with a temp `AID_HOME` and **isolated cwd**.
///
/// Project config is discovered by walking from the process working directory
/// for a git root with `.aid/project.toml`. Leaving cwd at the developer repo
/// root makes e2e tasks inherit that repo's verify/team/skills defaults. The
/// temp `AID_HOME` is not a git repo, so discovery finds nothing unless a test
/// opts into a project via [`aid_cmd_with_cwd`].
///
/// We deliberately do **not** add a product-level env opt-out for discovery
/// (`AID_NO_PROJECT` or similar): discovery-from-cwd is correct operator
/// behaviour, and an escape hatch would hide bugs that should call
/// `detect_project_in(explicit_path)` instead. Tests that need a project write
/// one under a temp git root and point cwd there; tests that need git without
/// project config use a temp git root with no `.aid/project.toml`.
pub(crate) fn aid_cmd_in(aid_home: &Path) -> Command {
    aid_cmd_with_cwd(aid_home, aid_home)
}

/// Like [`aid_cmd_in`], but runs with an explicit working directory.
///
/// Use this when a test intentionally wants project discovery (pass a temp
/// git root that contains `.aid/project.toml`) or needs relative path resolution
/// against a specific tree. Do not pass the developer repo root unless the test
/// is explicitly probing inheritance.
pub(crate) fn aid_cmd_with_cwd(aid_home: &Path, cwd: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd.env("AID_NO_DETACH", "1");
    // Clear orchestrator environment variables that would pollute tests.
    // Without this, a run executed under an aid task sees AID_TASK_ID and is
    // treated as a delegated child, which rejects --bg outright.
    for var in [
        "AID_GROUP",
        "AID_TASK_ID",
        "AID_TASK_DEPTH",
        "AID_TASK_NAME",
        "AID_ITERATION",
        "AID_PARENT_TASK_ID",
        "AID_CASCADE",
    ] {
        cmd.env_remove(var);
    }
    cmd.current_dir(cwd);
    cmd
}

/// Live relay rows (2026-10-09) for gpt-5.6-sol/luna, stamped fresh so the score and
/// price feeds never refresh from the network during a test.
#[allow(dead_code)]
pub fn seed_codex_leaderboard(home: &Path) {
    let scores = include_str!("../fixtures/leaderboard/e2e-codex-scores.json");
    let prices = include_str!("../fixtures/leaderboard/e2e-codex-prices.json");
    for (file, body) in [("scores.json", scores), ("prices.json", prices)] {
        let mut feed: serde_json::Value = serde_json::from_str(body).expect("leaderboard fixture");
        feed["built_at"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
        std::fs::write(home.join(file), feed.to_string()).expect("leaderboard cache");
    }
}

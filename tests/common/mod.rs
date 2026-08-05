// Shared integration-test command helpers for invoking the `aid` binary.
// Exports helpers that isolate subprocess state with a temp AID_HOME.
// Deps: std::process::Command and temp directories supplied by callers.

use std::path::Path;
use std::process::Command;

pub(crate) fn aid_cmd_in(aid_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd.env("AID_NO_DETACH", "1");
    // Clear orchestrator environment variables that would pollute tests
    cmd.env_remove("AID_GROUP");
    cmd.env_remove("AID_TASK_ID");
    cmd.env_remove("AID_TASK_DEPTH");
    cmd.env_remove("AID_TASK_NAME");
    cmd.env_remove("AID_ITERATION");
    cmd.env_remove("AID_PARENT_TASK_ID");
    cmd.env_remove("AID_CASCADE");
    
    // Project config is discovered from the working directory. Keeping cwd at the repo root makes
    // e2e tasks inherit ai-dispatch's own `.aid/project.toml`, including its verify command. The
    // temp AID_HOME is not a git repo, so discovery finds nothing unless a test opts into a project.
    cmd.current_dir(aid_home);
    cmd
}

// Grok permission flags for headless commands.
// Depends on Command and adapter run options.

use super::*;

pub(super) fn configure_permissions(cmd: &mut Command, opts: &RunOpts, allow_result: bool) {
    // Global Claude Code Stop hooks (`hiboss hook stop`) refuse the first
    // exit until `hiboss ask` returns — and `hiboss ask` blocks for a human.
    // Headless `-p` has no human, so the session writes zero bytes and the
    // watchdog reaps it (t-764b2a1d, 1200s). Measured: `--deny Bash(hiboss:*)`
    // lets grok refuse the gate by policy and exit normally. Adapter-local
    // because only this CLI speaks `--deny`; scoped to hiboss (not all Bash)
    // so normal tool use stays intact. The `:*` covers ask/hook/notify.
    cmd.args(["--deny", "Bash(hiboss:*)"]);
    if opts.read_only && !allow_result {
        cmd.args(["--permission-mode", "plan"]);
    } else {
        // Without this grok asks for approval it can never receive: in
        // headless `-p` mode it renders no prompt, it just abandons the tool
        // call, returns `stopReason: "cancelled"` and exits 0. Measured in a
        // scratch repo — "add a line to a.txt" left the file untouched and
        // still billed, and a real 32-turn run burned $1.04 before being
        // cancelled the same way. `--permission-mode auto` is NOT enough; it
        // cancels identically. Every other adapter passes its own form of
        // this (codex `--full-auto`, agy/claude `--dangerously-skip-
        // permissions`, droid `--skip-permissions-unsafe`, kilo `--auto`).
        // Result-file audits need the same write path with prompt-level RO.
        cmd.arg("--always-approve");
    }
}

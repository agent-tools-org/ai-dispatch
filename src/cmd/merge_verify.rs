// Verify command helpers for merge flows.
// Exports: run_post_merge_verify, run_verify_in_worktree.
// Deps: crate::agent, std::path::Path, std::process::Command.

use std::path::Path;
use std::process::Command;

pub(crate) fn run_verify_in_worktree(wt: &str, verify: Option<&str>) {
    let worktree_branch = Path::new(wt).file_name().and_then(|name| name.to_str());
    let cargo_target_dir = crate::agent::target_dir_for_worktree(worktree_branch);
    run_verify(wt, verify, cargo_target_dir.as_deref());
}

pub(crate) fn run_post_merge_verify(repo_dir: &str, verify: Option<&str>) {
    run_verify(repo_dir, verify, None);
}

fn run_verify(dir: &str, verify: Option<&str>, cargo_target_dir: Option<&str>) {
    let verify_parts = match verify {
        Some("auto") | None => vec!["cargo", "check"],
        Some(cmd) => cmd.split_whitespace().collect::<Vec<_>>(),
    };
    let Some((program, args)) = verify_parts.split_first() else {
        aid_warn!("[aid] Warning: verify command is empty");
        return;
    };
    let verify_cmd = verify_parts.join(" ");
    let mut command = Command::new(program);
    command.args(args).current_dir(dir);
    if let Some(target_dir) = cargo_target_dir {
        command.env("CARGO_TARGET_DIR", target_dir);
    }
    match command.output() {
        Ok(output) if !output.status.success() => warn_verify_failure(&verify_cmd, dir, &output.stderr),
        Err(err) => aid_warn!("[aid] Warning: could not run `{verify_cmd}`: {err}"),
        _ => {}
    }
}

fn warn_verify_failure(verify_cmd: &str, dir: &str, stderr: &[u8]) {
    aid_warn!("[aid] Warning: `{verify_cmd}` failed in {dir}");
    let stderr = String::from_utf8_lossy(stderr);
    for line in stderr.lines().take(5) {
        aid_warn!("  {}", line);
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn verify_argv_split_keeps_shell_tokens_literal() {
        let parts: Vec<&str> = "echo ok && false".split_whitespace().collect();
        let (program, args) = parts.split_first().unwrap();
        let cmd = Command::new(program);
        let debug = format!("{cmd:?}");
        assert!(debug.contains("\"echo\""));
        assert_eq!(args, &["ok", "&&", "false"]);
    }
}

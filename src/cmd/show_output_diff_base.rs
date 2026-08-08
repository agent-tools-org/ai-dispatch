// Git plumbing behind `aid show --diff`: branch-base resolution and diff invocation.
// Exports: branch_base_ref, merge_base, branch_diff_signal, generate_diff(_file), head_matches_start.
// Deps: git CLI via std::process.
use std::process::Command;

const DIFF_EXCLUDE: &[&str] = &[":(exclude)*.lock", ":(exclude)package-lock.json"];

/// A ref that actually resolves here, not a name that merely looks right.
/// `origin/HEAD` names the remote default (`origin/main`); the remote prefix has to
/// survive, because a clone that never created the local branch cannot resolve a bare
/// `main`, and `git merge-base main HEAD` then exits 128. Returning None is the honest
/// answer when no base can be found — callers must not print a guess as the branch.
pub(crate) fn branch_base_ref(wt_path: &str) -> Option<String> {
    if let Some(name) = git_line(wt_path, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        && ref_exists(wt_path, &name)
    {
        return Some(name);
    }
    ["main", "master", "origin/main", "origin/master"]
        .into_iter()
        .find(|name| ref_exists(wt_path, name))
        .map(str::to_string)
}

pub(super) fn merge_base(wt_path: &str, base_ref: &str) -> Option<String> {
    git_line(wt_path, &["merge-base", base_ref, "HEAD"])
}

/// A task dispatched into a worktree that already carries commits gets a baseline
/// above them, so `start_sha..HEAD` can be a two-line sliver while the branch holds
/// the delivered work. Say so, rather than letting the sliver read as the whole story.
pub(super) fn branch_diff_signal(
    wt_path: &str,
    start_sha: Option<&str>,
    base_ref: Option<&str>,
) -> Option<String> {
    let start = start_sha?;
    let base_sha = merge_base(wt_path, base_ref?)?;
    let start_sha_full = git_line(wt_path, &["rev-parse", start])?;
    if base_sha == start_sha_full {
        return None;
    }
    let totals = git_line(wt_path, &["diff", "--shortstat", &format!("{base_sha}..HEAD")])
        .unwrap_or_default();
    Some(format!(
        "  (This branch carries earlier commits that are not in the diff above. Whole branch: {totals}. Run `aid show <task-id> --diff --branch` to see all of it)\n"
    ))
}

/// Ranges to try, most specific first: the task's own baseline, then the whole branch,
/// then the working tree. A base that does not resolve contributes no range at all
/// rather than a failing one.
pub(super) fn range_arg_sets(
    start_sha: Option<&str>,
    base_ref: Option<&str>,
    stat: bool,
) -> Vec<Vec<String>> {
    let mut sets: Vec<Vec<String>> = Vec::with_capacity(3);
    for range in [
        start_sha.map(|sha| format!("{sha}..HEAD")),
        base_ref.map(|base| format!("{base}...HEAD")),
        None,
    ] {
        let mut args = vec!["diff".to_string()];
        args.extend(range);
        if stat {
            args.push("--stat".to_string());
        }
        sets.push(args);
    }
    sets
}

pub(super) fn generate_diff(wt_path: &str, args_sets: &[Vec<String>], fallback: &str) -> String {
    for args in args_sets {
        if let Some(output) = run_git_diff(wt_path, &diff_args(args))
            && !output.trim().is_empty()
        {
            return output;
        }
    }
    fallback.to_string()
}

pub(super) fn generate_diff_file(
    wt_path: &str,
    args_sets: &[Vec<String>],
    fallback: &str,
    file: &str,
) -> String {
    for args in args_sets {
        if let Some(output) = run_git_diff(wt_path, &diff_args_file(args, file))
            && !output.trim().is_empty()
        {
            return output;
        }
    }
    fallback.to_string()
}

pub(super) fn head_matches_start(wt_path: &str, start_sha: &str) -> bool {
    git_line(wt_path, &["rev-parse", "HEAD"]).is_some_and(|head| head == start_sha)
}

fn diff_args(base_args: &[String]) -> Vec<String> {
    let mut args = base_args.to_vec();
    args.push("--".to_string());
    args.push(".".to_string());
    args.extend(DIFF_EXCLUDE.iter().map(|value| value.to_string()));
    args
}

fn diff_args_file(base_args: &[String], file: &str) -> Vec<String> {
    let mut args = base_args.to_vec();
    args.push("--".to_string());
    args.push(file.to_string());
    args
}

fn run_git_diff(wt_path: &str, args: &[String]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(wt_path).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into())
}

fn ref_exists(wt_path: &str, name: &str) -> bool {
    Command::new("git")
        .args(["-C", wt_path, "rev-parse", "--verify", "--quiet"])
        .arg(format!("{name}^{{commit}}"))
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git_line(wt_path: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(["-C", wt_path]).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!line.is_empty()).then_some(line)
}

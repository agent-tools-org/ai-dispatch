// Raw delivery patches for machine consumers of `aid show --patch`.
// Resolves retry baselines and collects tracked/untracked bytes without index writes.

use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::{store::Store, types::Task};
use super::show_output::{branch_base_ref_using, merge_base_using};

const CONFIG: &[&str] = &[
    "-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false",
    "-c", "diff.relative=false", "-c", "core.quotePath=true",
];
const DIFF: &[&str] = &[
    "diff", "--no-ext-diff", "--no-color", "--no-textconv", "--no-relative",
    "--src-prefix=a/", "--dst-prefix=b/", "--submodule=diff", "--ignore-submodules=none",
];

pub(super) fn run(store: &Store, task_id: &str) -> Result<()> {
    let patch = collect(store, task_id)?;
    std::io::stdout().lock().write_all(&patch).context("write task patch")
}

pub(super) fn run_start_commit(store: &Store, task_id: &str) -> Result<()> {
    let task = store.get_task(task_id)?.with_context(|| format!("Task not found: {task_id}"))?;
    let root = checkout_root(&task)?;
    let mut first = task;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(first.id.to_string()) { bail!("Cycle in task retry chain"); }
        let Some(parent_id) = first.parent_task_id.as_deref() else { break; };
        first = store.get_task(parent_id)?
            .with_context(|| format!("Cannot determine start commit: missing ancestor task {parent_id}"))?;
    }
    let start = first.start_sha.as_deref()
        .with_context(|| format!("First attempt {} has no recorded start_sha", first.id))?;
    let commit = git_text(&root, &["rev-parse", "--verify", &format!("{start}^{{commit}}")])
        .with_context(|| format!("Cannot resolve first attempt {} start commit {start}", first.id))?;
    println!("{commit}");
    Ok(())
}

fn collect(store: &Store, task_id: &str) -> Result<Vec<u8>> {
    let task = store.get_task(task_id)?.with_context(|| format!("Task not found: {task_id}"))?;
    let root = checkout_root(&task)?;
    let base = patch_base(store, &task, &root)?;
    let output = git(&root).args(DIFF).arg(base).arg("--").output().context("run git tracked patch")?;
    let mut patch = checked(output, "tracked patch")?;
    let files = checked(git(&root).args(["ls-files", "--others", "--exclude-per-directory=.gitignore", "-z"])
        .output().context("list untracked files")?, "untracked file list")?;
    for file in files.split(|byte| *byte == 0).filter(|file| !file.is_empty()) {
        let parts: Vec<_> = file.split(|byte| *byte == b'/').collect();
        if parts.windows(3).any(|parts| parts[0] == b".aic" && parts[1] == b"audits") {
            continue;
        }
        if root.join(OsStr::from_bytes(file)).symlink_metadata()
            .context("inspect untracked patch path")?.is_dir()
        {
            // An empty patch reads as "no change", so what cannot be shown must not be skipped.
            bail!("Cannot represent untracked directory as a patch: {}; ignore or remove it",
                Path::new(OsStr::from_bytes(file)).display());
        }
        let output = git(&root).args(DIFF).args(["--no-index", "--", "/dev/null"])
            .arg(OsStr::from_bytes(file)).output().context("generate untracked patch")?;
        // Exit 1 means "differs", but a path git cannot read also exits 1, with no patch.
        if !output.status.success() && (output.status.code() != Some(1) || output.stdout.is_empty()) {
            bail!("Cannot represent untracked path as a patch: {}: {}; ignore or remove it",
                Path::new(OsStr::from_bytes(file)).display(), String::from_utf8_lossy(&output.stderr).trim());
        }
        patch.extend(output.stdout);
    }
    Ok(patch)
}

fn patch_base(store: &Store, task: &Task, root: &Path) -> Result<String> {
    let base = retry_base(store, task, root)?
        .context("No base can be determined for task patch: retry chain has no start_sha")?;
    if task.worktree_path.is_some()
        && let Some(base_ref) = branch_base_ref_using(|args| git_text(root, args).ok())
    {
        return common_base(root, &base, &common_base(root, &base_ref, "HEAD")?);
    }
    Ok(base)
}

/// Fold every recorded start in the chain. An ancestor's checkout may be gone, but its
/// start still bounds the delivery as long as this repository can name that commit.
fn retry_base(store: &Store, task: &Task, root: &Path) -> Result<Option<String>> {
    let mut current = task.clone();
    let mut base: Option<String> = None;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(current.id.to_string()) { bail!("Cycle in task retry chain"); }
        if let Some(start) = current.start_sha.as_deref() {
            let resolved = git_text(root, &["rev-parse", "--verify", &format!("{start}^{{commit}}")])
                .with_context(|| format!("Cannot resolve recorded task patch base {start}"));
            let start = match resolved {
                Ok(start) => start,
                Err(_) if current.id != task.id => break,
                Err(error) => return Err(error),
            };
            base = Some(match base {
                Some(previous) => common_base(root, &previous, &start)?,
                None => start,
            });
        }
        let Some(parent_id) = current.parent_task_id.as_deref() else { break; };
        let Some(parent) = store.get_task(parent_id)? else { break; };
        current = parent;
    }
    Ok(base)
}

fn common_base(root: &Path, left: &str, right: &str) -> Result<String> {
    merge_base_using(|args| git_text(root, args).ok(), left, right)
        .with_context(|| format!("No common ancestor for task patch bases {left} and {right}"))
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    let bytes = checked(git(root).args(args).output().context("run git patch query")?, "patch query")?;
    Ok(String::from_utf8(bytes).context("git patch query is not UTF-8")?.trim().to_string())
}

fn checkout_root(task: &Task) -> Result<PathBuf> {
    let checkout = task.worktree_path.as_deref().or(task.effective_dir.as_deref())
        .or(task.repo_path.as_deref()).context("Task has no recorded checkout")?;
    if !Path::new(checkout).is_dir() {
        bail!("Task checkout no longer exists: {checkout}");
    }
    let output = checked(git(Path::new(checkout)).args(["rev-parse", "--show-toplevel"])
        .output().context("locate git repository")?, "checkout repository resolution")?;
    let path = output.strip_suffix(b"\n").unwrap_or(&output);
    Path::new(OsStr::from_bytes(path)).canonicalize().context("resolve checkout root")
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT", "GIT_PREFIX"]
    {
        command.env_remove(key);
    }
    command.arg("-C").arg(root).args(CONFIG);
    command
}

fn checked(output: Output, operation: &str) -> Result<Vec<u8>> {
    if !output.status.success() {
        bail!("git {operation} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}

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
use super::{branch_base_ref, show_output::merge_base};

const CONFIG: &[&str] = &[
    "-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false",
    "-c", "diff.relative=false", "-c", "core.quotePath=true",
];
const DIFF: &[&str] = &[
    "diff", "--no-ext-diff", "--no-color", "--no-textconv", "--no-relative",
    "--src-prefix=a/", "--dst-prefix=b/", "--submodule=diff",
];

pub(super) fn run(store: &Store, task_id: &str) -> Result<()> {
    let patch = collect(store, task_id)?;
    std::io::stdout().lock().write_all(&patch).context("write task patch")
}

fn collect(store: &Store, task_id: &str) -> Result<Vec<u8>> {
    let task = store.get_task(task_id)?.with_context(|| format!("Task not found: {task_id}"))?;
    let root = checkout_root(&task)?;
    let base = patch_base(store, &task, &root)?;
    let output = git(&root).args(DIFF).arg(base).arg("--").output().context("run git tracked patch")?;
    let mut patch = checked(output, "tracked patch")?;
    let files = checked(git(&root).args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output().context("list untracked files")?, "untracked file list")?;
    for file in files.split(|byte| *byte == 0).filter(|file| !file.is_empty()) {
        // aic writes its reports into a .aic directory wherever it runs.
        if file.split(|byte| *byte == b'/').any(|part| part == b".aic") { continue; }
        let output = git(&root).args(DIFF).args(["--no-index", "--", "/dev/null"])
            .arg(OsStr::from_bytes(file)).output().context("generate untracked patch")?;
        if !output.status.success() && output.status.code() != Some(1) {
            bail!("git untracked patch failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        patch.extend(output.stdout);
    }
    Ok(patch)
}

fn patch_base(store: &Store, task: &Task, root: &Path) -> Result<String> {
    if task.worktree_path.is_some() {
        let path = root.to_str().context("checkout path is not UTF-8")?;
        if let Some(base_ref) = branch_base_ref(path) {
            return merge_base(path, &base_ref).context("git merge-base failed for task patch");
        }
    }
    let mut current = task.clone();
    let mut base = current.start_sha.clone();
    let mut seen = HashSet::new();
    seen.insert(current.id.to_string());
    while let Some(parent_id) = current.parent_task_id.as_deref() {
        if !seen.insert(parent_id.to_string()) { bail!("Cycle in task retry chain"); }
        let parent = store.get_task(parent_id)?
            .with_context(|| format!("Retry ancestor not found: {parent_id}"))?;
        if checkout_root(&parent)? != root { break; }
        if parent.start_sha.is_some() { base = parent.start_sha.clone(); }
        current = parent;
    }
    let base = base.context("No base can be determined for task patch: retry chain has no start_sha")?;
    let resolved = checked(git(root).args(["rev-parse", "--verify"])
        .arg(format!("{base}^{{commit}}")).output().context("resolve git patch base")?,
        "patch base resolution")?;
    Ok(String::from_utf8(resolved).context("git base is not UTF-8")?.trim().to_string())
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
    command.arg("--literal-pathspecs").arg("-C").arg(root).args(CONFIG);
    command
}

fn checked(output: Output, operation: &str) -> Result<Vec<u8>> {
    if !output.status.success() {
        bail!("git {operation} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}

// Proves accepted Git artifacts survive deletion of worktree-private object stores.
// Exports recursive manifest verification; depends only on Git and filesystem paths.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::acceptance::git_output;

#[derive(Debug, Serialize)]
pub(super) struct DurabilityCertificate {
    pub head_sha: String,
    pub manifest_digest: String,
    pub repositories: Vec<DurableRepository>,
}

#[derive(Debug, Serialize)]
pub(super) struct DurableRepository {
    pub path: String,
    pub commit: String,
    pub durable_git_dir: String,
}

pub(super) fn verify(worktree: &Path, accepted_head: &str) -> Result<DurabilityCertificate> {
    ensure_clean(worktree)?;
    let live_head = git_output(worktree, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        live_head == accepted_head,
        "Artifact changed after acceptance: accepted {accepted_head}, current {live_head}"
    );
    let common_dir = resolve_common_dir(worktree)?;
    let mut repositories = Vec::new();
    verify_repository(&common_dir, accepted_head, "", &mut repositories)?;
    Ok(DurabilityCertificate {
        head_sha: accepted_head.to_string(),
        manifest_digest: manifest_digest(worktree, accepted_head)?,
        repositories,
    })
}

pub(super) fn manifest_digest(worktree: &Path, head: &str) -> Result<String> {
    git_output(worktree, &["rev-parse", &format!("{head}^{{tree}}")])
}

fn ensure_clean(worktree: &Path) -> Result<()> {
    let status = git_output(worktree, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    anyhow::ensure!(status.is_empty(), "Artifact is dirty after acceptance:\n{status}");
    Ok(())
}

fn resolve_common_dir(worktree: &Path) -> Result<PathBuf> {
    let value = git_output(worktree, &["rev-parse", "--git-common-dir"])?;
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(worktree.join(path).canonicalize()?)
}

fn verify_repository(
    git_dir: &Path,
    commit: &str,
    logical_path: &str,
    found: &mut Vec<DurableRepository>,
) -> Result<()> {
    prove_object_and_ref(git_dir, commit, logical_path)?;
    found.push(DurableRepository {
        path: logical_path.to_string(),
        commit: commit.to_string(),
        durable_git_dir: git_dir.display().to_string(),
    });
    for (child_path, child_commit) in gitlinks(git_dir, commit)? {
        let child_git_dir = git_dir.join("modules").join(&child_path);
        let full_path = join_logical_path(logical_path, &child_path);
        verify_repository(&child_git_dir, &child_commit, &full_path, found)?;
    }
    Ok(())
}

fn prove_object_and_ref(git_dir: &Path, commit: &str, logical_path: &str) -> Result<()> {
    anyhow::ensure!(
        git_dir.is_dir(),
        "No durable object store for submodule '{logical_path}' at {}",
        git_dir.display()
    );
    run_git_dir(git_dir, &["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .with_context(|| format!("Commit {commit} for '{logical_path}' exists only in disposable storage"))?;
    let refs = run_git_dir(
        git_dir,
        &["for-each-ref", "--contains", commit, "--format=%(refname)"],
    )?;
    anyhow::ensure!(
        !refs.trim().is_empty(),
        "Commit {commit} for '{logical_path}' has no durable ref"
    );
    Ok(())
}

fn gitlinks(git_dir: &Path, commit: &str) -> Result<Vec<(String, String)>> {
    let output = run_git_dir(git_dir, &["ls-tree", "-r", commit])?;
    Ok(output
        .lines()
        .filter_map(parse_gitlink)
        .collect())
}

fn parse_gitlink(line: &str) -> Option<(String, String)> {
    let (metadata, path) = line.split_once('\t')?;
    let mut fields = metadata.split_whitespace();
    (fields.next()? == "160000").then(|| {
        let _kind = fields.next();
        let commit = fields.next().unwrap_or_default();
        (path.to_string(), commit.to_string())
    })
}

fn run_git_dir(git_dir: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg(format!("--git-dir={}", git_dir.display()))
        .args(args)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn join_logical_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::verify;
    use std::path::Path;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-c")
            .arg("protocol.file.allow=always")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init(repo: &Path) {
        std::fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["config", "user.email", "aid@example.invalid"]);
        git(repo, &["config", "user.name", "AID Test"]);
    }

    fn commit_file(repo: &Path, name: &str, value: &str, message: &str) {
        std::fs::write(repo.join(name), value).unwrap();
        git(repo, &["add", name]);
        git(repo, &["commit", "-m", message]);
    }

    #[test]
    fn ordinary_accepted_branch_is_durable() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("worktree");
        init(&repo);
        commit_file(&repo, "README.md", "base", "base");
        git(&repo, &["worktree", "add", "-b", "task/one", worktree.to_str().unwrap()]);
        commit_file(&worktree, "result.txt", "done", "result");
        let head = git(&worktree, &["rev-parse", "HEAD"]);

        let certificate = verify(&worktree, &head).unwrap();

        assert_eq!(certificate.head_sha, head);
        assert_eq!(certificate.repositories.len(), 1);
    }

    #[test]
    fn worktree_private_submodule_commit_is_not_durable() {
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("child");
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("worktree");
        init(&child);
        commit_file(&child, "rpc.rs", "base", "base");
        init(&repo);
        commit_file(&repo, "README.md", "base", "base");
        git(
            &repo,
            &["submodule", "add", child.to_str().unwrap(), "lib/fast-wallet"],
        );
        git(&repo, &["commit", "-am", "add submodule"]);
        git(&repo, &["worktree", "add", "-b", "fix/866", worktree.to_str().unwrap()]);
        git(&worktree, &["submodule", "update", "--init"]);
        let child_worktree = worktree.join("lib/fast-wallet");
        git(&child_worktree, &["config", "user.email", "aid@example.invalid"]);
        git(&child_worktree, &["config", "user.name", "AID Test"]);
        commit_file(&child_worktree, "rpc.rs", "fanout", "fanout");
        git(&worktree, &["add", "lib/fast-wallet"]);
        git(&worktree, &["commit", "-m", "advance gitlink"]);
        let head = git(&worktree, &["rev-parse", "HEAD"]);

        let error = verify(&worktree, &head).unwrap_err().to_string();

        assert!(error.contains("exists only in disposable storage"), "{error}");
        assert!(worktree.exists());
    }
}

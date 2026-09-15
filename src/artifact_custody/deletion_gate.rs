// Sole authorization gate for deletion of task-owned worktrees.
// Exports deletion after acceptance and recursive durability proof.

use anyhow::{Context, Result};
use std::path::Path;

use crate::store::{AcceptanceDecision, Store};

pub(crate) fn delete_accepted_worktree(store: &Store, task_id: &str) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .with_context(|| format!("Task not found: {task_id}"))?;
    let acceptance = store
        .latest_acceptance(task_id)?
        .with_context(|| format!("Task {task_id} has not been accepted by its principal"))?;
    anyhow::ensure!(
        acceptance.decision == AcceptanceDecision::Accepted,
        "Task {task_id} was rejected; its artifacts remain in custody"
    );
    let head = acceptance
        .accepted_head_sha
        .as_deref()
        .context("Acceptance record has no head commit")?;
    let worktree_str = task
        .worktree_path
        .as_deref()
        .context("Task has no worktree artifact")?;
    let worktree = Path::new(worktree_str);
    let repo_path_str = task
        .repo_path
        .as_deref()
        .context("Task has no repository")?;
    let repo_path = Path::new(repo_path_str);

    let is_missing = !worktree.exists() && is_worktree_missing(repo_path_str, worktree)?;

    let certificate = super::durability::verify(worktree, repo_path, head, is_missing)?;
    let digest = acceptance
        .manifest_digest
        .as_deref()
        .context("Acceptance record has no artifact manifest")?;
    anyhow::ensure!(
        certificate.manifest_digest == digest,
        "Artifact manifest changed after acceptance"
    );
    let certificate_json = serde_json::to_string(&certificate)?;
    store.record_durability(task_id, head, digest, &certificate_json)?;

    if is_missing {
        println!("Worktree was already collected");
        prune_worktrees(repo_path_str)?;
    } else {
        remove_worktree(repo_path_str, worktree)?;
    }

    let _ = crate::cmd::clean_cargo_target::remove_task_fallback_target_dirs(store, &task);
    Ok(())
}

fn is_worktree_missing(repo: &str, worktree: &Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["-C", repo, "worktree", "list", "--porcelain"])
        .output()
        .context("Failed to run git worktree list")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            if Path::new(path_str) == worktree {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn prune_worktrees(repo: &str) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["-C", repo, "worktree", "prune"])
        .output()
        .context("Failed to start git worktree prune")?;
    anyhow::ensure!(
        output.status.success(),
        "git worktree prune failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn remove_worktree(repo: &str, worktree: &Path) -> Result<()> {
    anyhow::ensure!(
        crate::worktree::is_aid_managed_worktree_path(worktree),
        "Refusing to delete non-AID worktree {}",
        worktree.display()
    );
    let output = std::process::Command::new("git")
        .args(["-C", repo, "worktree", "remove", "--force"])
        .arg(worktree)
        .output()
        .context("Failed to start git worktree remove")?;
    anyhow::ensure!(
        output.status.success(),
        "git worktree remove failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::delete_accepted_worktree;
    use crate::store::{AcceptanceDecision, AcceptanceRecord, Store};
    use rusqlite::params;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "protocol.file.allow=always", "-C"])
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test User"]);
        fs::write(dir.join(name), "base\n").unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-m", "base"]);
    }

    fn setup_task(store: &Store, task_id: &str, repo: &Path, wt: &Path, branch: &str) -> String {
        let head = git(wt, &["rev-parse", "HEAD"]);
        let manifest = git(wt, &["rev-parse", &format!("{head}^{{tree}}")]);
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, worktree_branch, created_at)
             VALUES (?1, 'codex', 'task', 'done', ?2, ?3, ?4, ?5)",
            params![task_id, repo.display().to_string(), wt.display().to_string(), branch, chrono::Local::now().to_rfc3339()]
        ).unwrap();
        store
            .record_acceptance(
                task_id,
                &AcceptanceRecord {
                    decision: AcceptanceDecision::Accepted,
                    principal_id: "t".into(),
                    accepted_head_sha: Some(head.clone()),
                    accepted_branch: Some(branch.into()),
                    manifest_digest: Some(manifest),
                },
                "cli",
            )
            .unwrap();
        head
    }

    #[test]
    fn completed_but_unaccepted_task_is_preserved() {
        let store = Store::open_memory().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, created_at)
             VALUES ('t-unaccepted', 'codex', 'task', 'done', ?1, ?2, ?3)",
            params![worktree.path().display().to_string(), worktree.path().display().to_string(), chrono::Local::now().to_rfc3339()]
        ).unwrap();
        assert!(delete_accepted_worktree(&store, "t-unaccepted")
            .unwrap_err()
            .to_string()
            .contains("has not been accepted"));
        assert!(worktree.path().exists());
    }

    #[test]
    fn accepted_worktree_deletion_reclaims_task_fallback_target_dir() {
        let store = Store::open_memory().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path(), "file.txt");
        let wt = crate::worktree::aid_worktree_root()
            .join("proj")
            .join("feat-test-gc");
        fs::create_dir_all(&wt).unwrap();
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                &wt.to_string_lossy(),
                "-b",
                "feat-test-gc",
            ],
        );
        setup_task(&store, "t-accepted-gc", repo.path(), &wt, "feat-test-gc");

        let fallback_root = tempfile::tempdir().unwrap();
        let fallback_dir = fallback_root
            .path()
            .join(crate::cmd::build::build_fallback::cwd_key(&wt));
        fs::create_dir_all(&fallback_dir).unwrap();
        fs::write(fallback_dir.join("artifact"), b"build-data").unwrap();
        let _guard = crate::test_env::FallbackTargetDirGuard::set(fallback_root.path());

        delete_accepted_worktree(&store, "t-accepted-gc").unwrap();
        assert!(!wt.exists());
        assert!(!fallback_dir.exists());
    }

    #[test]
    fn missing_worktree_must_not_skip_durability_proof() {
        let store = Store::open_memory().unwrap();
        let child = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(child.path(), "rpc.rs");
        init_repo(repo.path(), "README.md");
        git(
            repo.path(),
            &["submodule", "add", child.path().to_str().unwrap(), "lib/fw"],
        );
        git(repo.path(), &["commit", "-am", "submodule"]);

        let wt = tempfile::tempdir().unwrap().path().join("feat-sub");
        git(
            repo.path(),
            &["worktree", "add", wt.to_str().unwrap(), "-b", "feat-sub"],
        );
        git(&wt, &["submodule", "update", "--init"]);
        git(
            &wt.join("lib/fw"),
            &["config", "user.email", "test@example.com"],
        );
        git(&wt.join("lib/fw"), &["config", "user.name", "Test User"]);
        fs::write(wt.join("lib/fw/rpc.rs"), "fanout").unwrap();
        git(&wt.join("lib/fw"), &["commit", "-am", "fanout"]);
        git(&wt, &["add", "lib/fw"]);
        git(&wt, &["commit", "-m", "advance gitlink"]);

        setup_task(&store, "t-sub", repo.path(), &wt, "feat-sub");
        fs::remove_dir_all(&wt).unwrap();
        git(repo.path(), &["worktree", "prune"]);

        let err = delete_accepted_worktree(&store, "t-sub")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("exists only in disposable storage")
                || err.contains("fatal: Not a valid object name")
        );
    }

    #[test]
    fn present_but_dirty_worktree_must_error() {
        let store = Store::open_memory().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path(), "README.md");
        let wt = tempfile::tempdir().unwrap().path().join("feat-dirty");
        git(
            repo.path(),
            &["worktree", "add", wt.to_str().unwrap(), "-b", "feat-dirty"],
        );
        setup_task(&store, "t-dirty", repo.path(), &wt, "feat-dirty");

        fs::write(wt.join("untracked.txt"), "oops").unwrap();
        assert!(delete_accepted_worktree(&store, "t-dirty")
            .unwrap_err()
            .to_string()
            .contains("Artifact is dirty after acceptance"));
    }
}

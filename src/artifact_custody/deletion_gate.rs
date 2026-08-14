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
    let worktree = task
        .worktree_path
        .as_deref()
        .map(Path::new)
        .context("Task has no worktree artifact")?;
    let certificate = super::durability::verify(worktree, head)?;
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
    remove_worktree(
        task.repo_path.as_deref().context("Task has no repository")?,
        worktree,
    )?;
    let _ = crate::cmd::clean_cargo_target::remove_task_fallback_target_dirs(store, &task);
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
    use crate::store::Store;
    use rusqlite::params;

    #[test]
    fn completed_but_unaccepted_task_is_preserved() {
        let store = Store::open_memory().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        store
            .db()
            .execute(
                "INSERT INTO tasks
                 (id, agent, prompt, status, repo_path, worktree_path, created_at)
                 VALUES (?1, 'codex', 'task', 'done', ?2, ?3, ?4)",
                params![
                    "t-unaccepted",
                    worktree.path().display().to_string(),
                    worktree.path().display().to_string(),
                    chrono::Local::now().to_rfc3339()
                ],
            )
            .unwrap();

        let error = delete_accepted_worktree(&store, "t-unaccepted")
            .unwrap_err()
            .to_string();

        assert!(error.contains("has not been accepted"), "{error}");
        assert!(worktree.path().exists());
    }

    #[test]
    fn accepted_worktree_deletion_reclaims_task_fallback_target_dir() {
        use std::fs;
        use crate::store::{AcceptanceDecision, AcceptanceRecord};

        let store = Store::open_memory().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-b", "main", &repo.path().to_string_lossy()])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", &repo.path().to_string_lossy(), "config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", &repo.path().to_string_lossy(), "config", "user.name", "Test User"])
            .status()
            .unwrap();
        fs::write(repo.path().join("file.txt"), "base\n").unwrap();
        std::process::Command::new("git")
            .args(["-C", &repo.path().to_string_lossy(), "add", "file.txt"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", &repo.path().to_string_lossy(), "commit", "-m", "base"])
            .status()
            .unwrap();

        let wt = crate::worktree::aid_worktree_root().join("proj").join("feat-test-gc");
        fs::create_dir_all(&wt).unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                &repo.path().to_string_lossy(),
                "worktree",
                "add",
                &wt.to_string_lossy(),
                "-b",
                "feat-test-gc",
            ])
            .status()
            .unwrap();

        let head = crate::artifact_custody::acceptance::git_output(&wt, &["rev-parse", "HEAD"]).unwrap();
        let manifest = crate::artifact_custody::durability::manifest_digest(&wt, &head).unwrap();

        store
            .db()
            .execute(
                "INSERT INTO tasks
                 (id, agent, prompt, status, repo_path, worktree_path, worktree_branch, created_at)
                 VALUES ('t-accepted-gc', 'codex', 'task', 'done', ?1, ?2, 'feat-test-gc', ?3)",
                params![
                    repo.path().display().to_string(),
                    wt.display().to_string(),
                    chrono::Local::now().to_rfc3339()
                ],
            )
            .unwrap();

        store
            .record_acceptance(
                "t-accepted-gc",
                &AcceptanceRecord {
                    decision: AcceptanceDecision::Accepted,
                    principal_id: "test".to_string(),
                    accepted_head_sha: Some(head),
                    accepted_branch: Some("feat-test-gc".to_string()),
                    manifest_digest: Some(manifest),
                },
                "cli",
            )
            .unwrap();

        let fallback_root = tempfile::tempdir().unwrap();
        let fallback_dir = fallback_root
            .path()
            .join(crate::cmd::build::build_fallback::cwd_key(&wt));
        fs::create_dir_all(&fallback_dir).unwrap();
        fs::write(fallback_dir.join("artifact"), b"build-data").unwrap();
        fs::write(fallback_dir.join(".cargo-lock"), b"").unwrap();

        let _fallback_guard = crate::test_env::FallbackTargetDirGuard::set(fallback_root.path());

        delete_accepted_worktree(&store, "t-accepted-gc").unwrap();

        assert!(!wt.exists(), "Worktree must be removed");
        assert!(!fallback_dir.exists(), "Task fallback target dir must be removed on GC");
    }
}

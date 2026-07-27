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
    )
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
}

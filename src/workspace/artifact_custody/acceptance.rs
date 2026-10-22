// Records explicit principal decisions without mutating task artifacts.
// Exports accept and reject; depends on Store, task state, and Git.

use anyhow::{Context, Result};
use std::path::Path;

use crate::store::{AcceptanceDecision, AcceptanceRecord, Store};

pub(crate) fn accept(store: &Store, task_id: &str, principal_id: &str) -> Result<()> {
    if store
        .latest_acceptance(task_id)?
        .is_some_and(|record| record.decision == AcceptanceDecision::Accepted)
    {
        return Ok(());
    }
    let task = terminal_task(store, task_id)?;
    let artifact = task
        .worktree_path
        .as_deref()
        .map(Path::new)
        .map(|worktree| {
            if worktree.try_exists()? {
                let head = git_output(worktree, &["rev-parse", "HEAD"])?;
                let branch = Some(git_output(worktree, &["branch", "--show-current"])?);
                let manifest = super::durability::manifest_digest(worktree, &head)?;
                Ok((head, branch, manifest))
            } else {
                missing_worktree_artifact(&task, worktree).with_context(|| {
                    format!(
                        "Cannot accept task {task_id}: worktree {} is missing",
                        worktree.display()
                    )
                })
            }
        })
        .transpose()?;
    let (head, branch, manifest) = artifact
        .map(|(head, branch, manifest)| (Some(head), branch, Some(manifest)))
        .unwrap_or((None, None, None));
    store.record_acceptance(
        task_id,
        &AcceptanceRecord {
            decision: AcceptanceDecision::Accepted,
            principal_id: principal_id.to_string(),
            accepted_head_sha: head,
            accepted_branch: branch,
            manifest_digest: manifest,
        },
        "cli",
    )
}

fn missing_worktree_artifact(
    task: &crate::types::Task,
    worktree: &Path,
) -> Result<(String, Option<String>, String)> {
    let head = task
        .final_head_sha
        .as_deref()
        .context("Task has no recorded final commit")?;
    anyhow::ensure!(
        matches!(head.len(), 40 | 64) && head.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Task has no valid recorded final commit SHA: {head}"
    );
    let repo = task
        .repo_path
        .as_deref()
        .context("Task has no repository for durability proof")?;
    anyhow::ensure!(
        super::deletion_gate::is_worktree_missing(repo, worktree)?,
        "Missing worktree is still registered; its artifact custody is unresolved"
    );
    let certificate = super::durability::verify(worktree, Path::new(repo), head, true)?;
    Ok((
        certificate.head_sha,
        task.final_branch.clone(),
        certificate.manifest_digest,
    ))
}

pub(crate) fn reject(store: &Store, task_id: &str, principal_id: &str) -> Result<()> {
    if store
        .latest_acceptance(task_id)?
        .is_some_and(|record| record.decision == AcceptanceDecision::Rejected)
    {
        return Ok(());
    }
    terminal_task(store, task_id)?;
    store.record_acceptance(
        task_id,
        &AcceptanceRecord {
            decision: AcceptanceDecision::Rejected,
            principal_id: principal_id.to_string(),
            accepted_head_sha: None,
            accepted_branch: None,
            manifest_digest: None,
        },
        "cli",
    )
}

fn terminal_task(store: &Store, task_id: &str) -> Result<crate::types::Task> {
    let task = store
        .get_task(task_id)?
        .with_context(|| format!("Task not found: {task_id}"))?;
    anyhow::ensure!(
        task.status.is_terminal(),
        "Task {task_id} is not complete; principal acceptance is not allowed"
    );
    Ok(task)
}

pub(super) fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{accept, reject};
    use crate::store::{AcceptanceDecision, AcceptanceRecord, Store};
    use crate::workspace::artifact_custody::delete_accepted_worktree;
    use rusqlite::params;
    use std::fs;

    #[test]
    fn accept_and_gc_without_worktree_preserves_repository() {
        let store = Store::open_memory().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let missing_cwd = repo.path().join("removed-cwd");
        fs::write(repo.path().join("sentinel"), "preserve me").unwrap();
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, effective_dir, created_at)
             VALUES ('t-no-worktree', 'codex', 'audit', 'done', ?1, ?2, ?3)",
            params![repo.path().to_str(), missing_cwd.to_str(), chrono::Local::now().to_rfc3339()],
        ).unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let target = fallback
            .path()
            .join(crate::cmd::build::build_fallback::cwd_key(&missing_cwd));
        fs::create_dir(&target).unwrap();
        fs::write(target.join("artifact"), "build data").unwrap();
        let live_target = fallback
            .path()
            .join(crate::cmd::build::build_fallback::cwd_key(repo.path()));
        fs::create_dir(&live_target).unwrap();
        let _guard = crate::support::test_env::FallbackTargetDirGuard::set(fallback.path());

        accept(&store, "t-no-worktree", "principal").unwrap();
        accept(&store, "t-no-worktree", "another-principal").unwrap();
        let record = store.latest_acceptance("t-no-worktree").unwrap().unwrap();
        assert_eq!(record.decision, AcceptanceDecision::Accepted);
        assert_eq!(record.principal_id, "principal");
        assert!(record.accepted_head_sha.is_none());
        assert!(record.accepted_branch.is_none());
        assert!(record.manifest_digest.is_none());
        delete_accepted_worktree(&store, "t-no-worktree").unwrap();
        delete_accepted_worktree(&store, "t-no-worktree").unwrap();
        assert!(!target.exists());
        assert!(live_target.exists());
        assert_eq!(
            fs::read_to_string(repo.path().join("sentinel")).unwrap(),
            "preserve me"
        );

        reject(&store, "t-no-worktree", "principal").unwrap();
        assert!(delete_accepted_worktree(&store, "t-no-worktree")
            .unwrap_err()
            .to_string()
            .contains("was rejected"));
    }

    fn init_repo(repo: &std::path::Path) {
        super::git_output(repo, &["init", "-b", "main"]).unwrap();
        super::git_output(
            repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
    }

    #[test]
    fn headless_acceptance_cannot_collect_recorded_worktree() {
        let store = Store::open_memory().unwrap();
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        let wt = repo.path().join("worktree");
        super::git_output(
            repo.path(),
            &["worktree", "add", wt.to_str().unwrap(), "-b", "task"],
        )
        .unwrap();
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, worktree_path, created_at)
             VALUES ('t-headless', 'codex', 'task', 'done', ?1, ?2, ?3)",
            params![repo.path().to_str(), wt.to_str(), chrono::Local::now().to_rfc3339()],
        ).unwrap();
        store
            .record_acceptance(
                "t-headless",
                &AcceptanceRecord {
                    decision: AcceptanceDecision::Accepted,
                    principal_id: "principal".into(),
                    accepted_head_sha: None,
                    accepted_branch: None,
                    manifest_digest: None,
                },
                "cli",
            )
            .unwrap();
        assert!(delete_accepted_worktree(&store, "t-headless")
            .unwrap_err()
            .to_string()
            .contains("Acceptance record has no head commit"));
        assert!(wt.join(".git").exists());
        assert!(
            super::git_output(repo.path(), &["worktree", "list", "--porcelain"])
                .unwrap()
                .contains(wt.to_str().unwrap())
        );
    }
}

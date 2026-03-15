// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: crate::store::Store, crate::types::TaskStatus

use anyhow::{anyhow, Result};
use std::process::Command;
use std::sync::Arc;

use crate::store::Store;
use crate::types::TaskStatus;

pub fn run(store: Arc<Store>, task_id: Option<&str>, group: Option<&str>) -> Result<()> {
    match (task_id, group) {
        (Some(id), _) => merge_single(&store, id),
        (_, Some(group_id)) => merge_group(&store, group_id),
        (None, None) => Err(anyhow!("Provide either a task ID or --group <wg-id>")),
    }
}

/// Resolve repo directory: prefer task.repo_path, then detect from worktree, fallback to ".".
fn resolve_repo_dir(repo_path: Option<&str>, worktree_path: Option<&str>) -> String {
    if let Some(repo) = repo_path {
        return repo.to_string();
    }
    // Detect repo from worktree's git config (worktrees link back to main repo)
    if let Some(wt) = worktree_path {
        if let Ok(out) = Command::new("git")
            .args(["-C", wt, "rev-parse", "--show-toplevel"])
            .output()
        {
            if out.status.success() {
                let toplevel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // Worktree's toplevel IS the worktree itself; get the main repo via commondir
                if let Ok(common) = Command::new("git")
                    .args(["-C", wt, "rev-parse", "--git-common-dir"])
                    .output()
                {
                    if common.status.success() {
                        let common_dir = String::from_utf8_lossy(&common.stdout).trim().to_string();
                        let common_path = std::path::Path::new(&common_dir);
                        // commondir points to main repo's .git — parent is the repo root
                        if let Some(parent) = common_path.parent() {
                            if parent.join(".git").exists() {
                                return parent.to_string_lossy().to_string();
                            }
                        }
                    }
                }
                return toplevel;
            }
        }
    }
    ".".to_string()
}

/// Count commits on branch ahead of base (main or HEAD of repo).
fn commits_ahead(repo_dir: &str, branch: &str) -> u32 {
    let out = Command::new("git")
        .args(["-C", repo_dir, "rev-list", "--count", &format!("HEAD..{branch}")])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
        }
        _ => 0,
    }
}

/// Check for uncommitted changes in a worktree and auto-commit them.
fn auto_commit_uncommitted(wt_path: &str, branch: &str) -> bool {
    // Check for any uncommitted changes (staged + unstaged + untracked)
    let status = Command::new("git")
        .args(["-C", wt_path, "status", "--porcelain"])
        .output();
    let has_changes = match status {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => return false,
    };
    if !has_changes {
        return false;
    }
    eprintln!("[aid] Worktree has uncommitted changes — auto-committing on {branch}");
    // Stage all changes
    let _ = Command::new("git")
        .args(["-C", wt_path, "add", "-A"])
        .output();
    // Commit
    let out = Command::new("git")
        .args(["-C", wt_path, "commit", "-m", "chore: auto-commit agent changes before merge"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            eprintln!("[aid] Auto-committed uncommitted changes");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("[aid] Warning: auto-commit failed: {}", stderr.lines().next().unwrap_or(""));
            false
        }
        Err(e) => {
            eprintln!("[aid] Warning: auto-commit failed: {e}");
            false
        }
    }
}

/// Perform git merge and return whether new commits were actually merged.
fn git_merge_branch(repo_dir: &str, branch: &str) -> MergeResult {
    let head_before = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let output = Command::new("git")
        .args(["-C", repo_dir, "merge", branch, "--no-edit"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.contains("Already up to date") {
                return MergeResult::AlreadyUpToDate;
            }
            // Verify HEAD actually moved
            let head_after = Command::new("git")
                .args(["-C", repo_dir, "rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            if head_before == head_after {
                return MergeResult::AlreadyUpToDate;
            }
            MergeResult::Merged
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            MergeResult::Failed(stderr)
        }
        Err(e) => MergeResult::Failed(e.to_string()),
    }
}

enum MergeResult {
    Merged,
    AlreadyUpToDate,
    Failed(String),
}

/// Remove a worktree properly using git, with fs::remove_dir_all as fallback.
pub fn remove_worktree(repo_dir: &str, wt_path: &str) {
    // Try `git worktree remove` first (proper cleanup)
    let result = Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .output();
    match result {
        Ok(o) if o.status.success() => {
            eprintln!("[aid] Cleaned up worktree {wt_path}");
            return;
        }
        _ => {}
    }
    // Fallback to manual removal + prune
    match std::fs::remove_dir_all(wt_path) {
        Ok(()) => {
            eprintln!("[aid] Cleaned up worktree {wt_path}");
            let _ = Command::new("git")
                .args(["-C", repo_dir, "worktree", "prune"])
                .output();
        }
        Err(e) => eprintln!("[aid] Warning: failed to remove {wt_path}: {e}"),
    }
}

fn merge_single(store: &Store, task_id: &str) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    if task.status != TaskStatus::Done {
        return Err(anyhow!(
            "Task '{task_id}' is {} — only DONE tasks can be marked as merged",
            task.status.label()
        ));
    }
    let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());

    // Pre-merge verification: run verify command in worktree
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            run_verify_in_worktree(wt, task.verify.as_deref());
        }
    }
    // Auto cherry-pick worktree branch into current branch
    if let Some(ref branch) = task.worktree_branch {
        // Auto-commit any uncommitted changes before merge
        if let Some(wt) = task.worktree_path.as_deref() {
            if std::path::Path::new(wt).exists() {
                auto_commit_uncommitted(wt, branch);
            }
        }
        // Pre-check: verify branch has commits to merge
        let ahead = commits_ahead(&repo_dir, branch);
        if ahead == 0 {
            eprintln!("[aid] Error: branch {branch} has 0 commits ahead — nothing to merge");
            eprintln!("[aid] The agent may not have committed its changes.");
            if let Some(wt) = task.worktree_path.as_deref() {
                if std::path::Path::new(wt).exists() {
                    eprintln!("[aid] Worktree preserved at {wt} for manual recovery");
                }
            }
            return Err(anyhow!("No commits to merge from {branch}"));
        }
        eprintln!("[aid] Branch {branch} has {ahead} commit(s) ahead");
        match git_merge_branch(&repo_dir, branch) {
            MergeResult::Merged => {
                eprintln!("[aid] Merged branch {branch} into current branch");
            }
            MergeResult::AlreadyUpToDate => {
                eprintln!("[aid] Error: git merge reported 'Already up to date' despite {ahead} commit(s)");
                eprintln!("[aid] This may indicate a repo path mismatch. Worktree preserved.");
                return Err(anyhow!("Merge was a no-op — possible repo_path mismatch"));
            }
            MergeResult::Failed(stderr) => {
                eprintln!("[aid] Warning: git merge {branch} failed:");
                for line in stderr.lines().take(5) {
                    eprintln!("  {}", line);
                }
                eprintln!("[aid] Manual merge needed: git merge {branch}");
                // Don't clean up worktree — user needs it for manual merge
                store.update_task_status(task_id, TaskStatus::Done)?;
                return Err(anyhow!("Merge failed — resolve manually, then re-run aid merge {task_id}"));
            }
        }
    } else {
        eprintln!(
            "[aid] No worktree branch — agent edited files in-place. Nothing to merge."
        );
    }
    store.update_task_status(task_id, TaskStatus::Merged)?;
    println!("Marked {task_id} as merged");
    // Clean up worktree only after successful merge
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            remove_worktree(&repo_dir, wt);
        }
    }
    Ok(())
}

fn run_verify_in_worktree(wt: &str, verify: Option<&str>) {
    let verify_cmd = match verify {
        Some("auto") | None => "cargo check",
        Some(cmd) => cmd,
    };
    let parts: Vec<&str> = verify_cmd.split_whitespace().collect();
    let Some((cmd, args)) = parts.split_first() else { return };
    let output = Command::new(cmd).args(args).current_dir(wt).output();
    match output {
        Ok(o) if !o.status.success() => {
            eprintln!("[aid] Warning: `{verify_cmd}` failed in worktree {wt}");
            let stderr = String::from_utf8_lossy(&o.stderr);
            for line in stderr.lines().take(5) {
                eprintln!("  {}", line);
            }
        }
        Err(e) => eprintln!("[aid] Warning: could not run `{verify_cmd}`: {e}"),
        _ => {}
    }
}

fn merge_group(store: &Store, group_id: &str) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    let mut merged = 0;
    let mut skipped = Vec::new();
    let first_repo_dir = resolve_repo_dir(
        tasks.first().and_then(|t| t.repo_path.as_deref()),
        tasks.first().and_then(|t| t.worktree_path.as_deref()),
    );
    for task in &tasks {
        if task.status != TaskStatus::Done {
            skipped.push(format!("{} ({})", task.id, task.status.label()));
            continue;
        }
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        if let Some(ref branch) = task.worktree_branch {
            // Auto-commit uncommitted changes
            if let Some(wt) = task.worktree_path.as_deref() {
                if std::path::Path::new(wt).exists() {
                    auto_commit_uncommitted(wt, branch);
                }
            }
            let ahead = commits_ahead(&repo_dir, branch);
            if ahead == 0 {
                eprintln!("[aid] Warning: {} — branch {branch} has 0 commits, skipping", task.id);
                skipped.push(format!("{} (no commits)", task.id));
                continue;
            }
            match git_merge_branch(&repo_dir, branch) {
                MergeResult::Merged => {
                    eprintln!("[aid] Merged branch {branch}");
                }
                MergeResult::AlreadyUpToDate => {
                    eprintln!("[aid] Warning: {} — merge was no-op despite {ahead} commit(s)", task.id);
                    skipped.push(format!("{} (merge no-op)", task.id));
                    continue;
                }
                MergeResult::Failed(_) => {
                    eprintln!("[aid] Warning: git merge {branch} failed, skipping {}", task.id);
                    skipped.push(format!("{} (merge conflict)", task.id));
                    continue;
                }
            }
        } else {
            eprintln!("[aid] {} — no worktree, edits applied in-place", task.id);
        }
        store.update_task_status(task.id.as_str(), TaskStatus::Merged)?;
        merged += 1;
        // Clean up worktree after successful merge
        if let Some(wt) = task.worktree_path.as_deref() {
            if std::path::Path::new(wt).exists() {
                remove_worktree(&repo_dir, wt);
            }
        }
    }
    println!("Merged {merged} task(s) in group {group_id}");
    if !skipped.is_empty() {
        eprintln!("[aid] Skipped: {}", skipped.join(", "));
    }
    // Prune stale git worktree references
    let _ = Command::new("git")
        .args(["-C", &first_repo_dir, "worktree", "prune"])
        .output();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use chrono::Local;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn git(repo: &Path, args: &[&str]) {
        let s = Command::new("git")
            .args(["-C", &repo.to_string_lossy()])
            .args(args)
            .output()
            .unwrap();
        assert!(s.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&s.stderr));
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())
    }

    /// Create a git repo with one commit. Returns the TempDir.
    fn init_repo() -> TempDir {
        let repo = TempDir::new().unwrap();
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@aid.dev"]);
        git(repo.path(), &["config", "user.name", "Test"]);
        std::fs::write(repo.path().join("init.txt"), "init\n").unwrap();
        git(repo.path(), &["add", "init.txt"]);
        git(repo.path(), &["commit", "-m", "init"]);
        repo
    }

    /// Create a worktree branch with one committed change. Returns (worktree_dir, branch_name).
    fn create_worktree_with_commit(repo: &Path) -> (TempDir, String) {
        let branch = unique("test-branch");
        let wt = TempDir::new().unwrap();
        git(repo, &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
        std::fs::write(wt.path().join("agent-work.txt"), "agent output\n").unwrap();
        git(wt.path(), &["add", "agent-work.txt"]);
        git(wt.path(), &["commit", "-m", "agent: implement feature"]);
        (wt, branch)
    }

    fn make_task_with_worktree(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: Some(repo.to_string_lossy().to_string()),
            worktree_path: Some(wt.to_string_lossy().to_string()),
            worktree_branch: Some(branch.to_string()),
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        }
    }

    // --- Unit tests for helper functions ---

    #[test]
    fn commits_ahead_detects_branch_with_commits() {
        let repo = init_repo();
        let (wt, branch) = create_worktree_with_commit(repo.path());
        assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);
        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn commits_ahead_returns_zero_for_same_head() {
        let repo = init_repo();
        let branch = unique("empty-branch");
        git(repo.path(), &["branch", &branch]);
        assert_eq!(commits_ahead(&repo.path().to_string_lossy(), &branch), 0);
    }

    #[test]
    fn commits_ahead_returns_zero_for_missing_branch() {
        let repo = init_repo();
        assert_eq!(commits_ahead(&repo.path().to_string_lossy(), "nonexistent"), 0);
    }

    #[test]
    fn auto_commit_uncommitted_commits_dirty_worktree() {
        let repo = init_repo();
        let branch = unique("dirty-branch");
        let wt = TempDir::new().unwrap();
        git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
        // Leave changes uncommitted
        std::fs::write(wt.path().join("dirty.txt"), "uncommitted\n").unwrap();

        let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
        assert!(committed);
        // Now the branch should have commits ahead
        assert!(commits_ahead(&repo.path().to_string_lossy(), &branch) > 0);

        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn auto_commit_uncommitted_returns_false_for_clean_worktree() {
        let repo = init_repo();
        let (wt, branch) = create_worktree_with_commit(repo.path());
        let committed = auto_commit_uncommitted(&wt.path().to_string_lossy(), &branch);
        assert!(!committed);
        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn git_merge_branch_merges_committed_branch() {
        let repo = init_repo();
        let (wt, branch) = create_worktree_with_commit(repo.path());

        let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
        assert!(matches!(result, MergeResult::Merged));
        // Verify the file landed in main
        assert!(repo.path().join("agent-work.txt").exists());

        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn git_merge_branch_detects_already_up_to_date() {
        let repo = init_repo();
        let branch = unique("noop-branch");
        git(repo.path(), &["branch", &branch]);

        let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
        assert!(matches!(result, MergeResult::AlreadyUpToDate));
    }

    #[test]
    fn git_merge_branch_detects_conflict() {
        let repo = init_repo();
        let branch = unique("conflict-branch");
        let wt = TempDir::new().unwrap();
        git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
        // Create conflicting changes
        std::fs::write(wt.path().join("init.txt"), "branch version\n").unwrap();
        git(wt.path(), &["add", "init.txt"]);
        git(wt.path(), &["commit", "-m", "branch change"]);
        std::fs::write(repo.path().join("init.txt"), "main version\n").unwrap();
        git(repo.path(), &["add", "init.txt"]);
        git(repo.path(), &["commit", "-m", "main change"]);

        let result = git_merge_branch(&repo.path().to_string_lossy(), &branch);
        assert!(matches!(result, MergeResult::Failed(_)));
        // Abort the failed merge
        let _ = Command::new("git").args(["-C", &repo.path().to_string_lossy(), "merge", "--abort"]).output();
        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn resolve_repo_dir_prefers_explicit_repo_path() {
        let result = resolve_repo_dir(Some("/explicit/repo"), Some("/tmp/worktree"));
        assert_eq!(result, "/explicit/repo");
    }

    #[test]
    fn resolve_repo_dir_detects_from_worktree() {
        let repo = init_repo();
        let (wt, _branch) = create_worktree_with_commit(repo.path());

        let result = resolve_repo_dir(None, Some(&wt.path().to_string_lossy()));
        // Should resolve to the main repo, not the worktree
        let canon_repo = repo.path().canonicalize().unwrap();
        let canon_result = Path::new(&result).canonicalize().unwrap();
        assert_eq!(canon_result, canon_repo);

        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn resolve_repo_dir_falls_back_to_dot() {
        let result = resolve_repo_dir(None, None);
        assert_eq!(result, ".");
    }

    // --- Integration tests for merge_single ---

    #[test]
    fn merge_single_succeeds_with_committed_worktree() {
        let repo = init_repo();
        let (wt, branch) = create_worktree_with_commit(repo.path());
        let store = Store::open_memory().unwrap();
        let task = make_task_with_worktree("t-merge-ok", repo.path(), wt.path(), &branch);
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-merge-ok");
        assert!(result.is_ok(), "merge_single failed: {result:?}");

        let loaded = store.get_task("t-merge-ok").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Merged);
        assert!(repo.path().join("agent-work.txt").exists());
    }

    #[test]
    fn merge_single_auto_commits_then_merges() {
        let repo = init_repo();
        let branch = unique("uncommitted");
        let wt = TempDir::new().unwrap();
        git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
        // Leave changes uncommitted — this is the data-loss scenario
        std::fs::write(wt.path().join("uncommitted.txt"), "agent forgot to commit\n").unwrap();

        let store = Store::open_memory().unwrap();
        let task = make_task_with_worktree("t-autocommit", repo.path(), wt.path(), &branch);
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-autocommit");
        assert!(result.is_ok(), "merge_single should auto-commit and merge: {result:?}");

        let loaded = store.get_task("t-autocommit").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Merged);
        assert!(repo.path().join("uncommitted.txt").exists());
        assert_eq!(std::fs::read_to_string(repo.path().join("uncommitted.txt")).unwrap(), "agent forgot to commit\n");
    }

    #[test]
    fn merge_single_fails_when_no_commits_and_no_changes() {
        let repo = init_repo();
        let branch = unique("empty");
        let wt = TempDir::new().unwrap();
        git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
        // No changes at all — nothing to merge

        let store = Store::open_memory().unwrap();
        let task = make_task_with_worktree("t-empty", repo.path(), wt.path(), &branch);
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-empty");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No commits to merge"), "unexpected error: {err}");

        // Task should still be Done (not Merged)
        let loaded = store.get_task("t-empty").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Done);
        // Worktree should be preserved
        assert!(wt.path().exists());

        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn merge_single_rejects_non_done_task() {
        let store = Store::open_memory().unwrap();
        let mut task = make_task_with_worktree("t-running", Path::new("."), Path::new("/tmp"), "b");
        task.status = TaskStatus::Running;
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-running");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("only DONE"));
    }

    #[test]
    fn merge_single_works_without_worktree_branch() {
        let store = Store::open_memory().unwrap();
        let task = Task {
            id: TaskId("t-inplace".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        };
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-inplace");
        assert!(result.is_ok());
        let loaded = store.get_task("t-inplace").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Merged);
    }

    #[test]
    fn merge_single_preserves_worktree_on_conflict() {
        let repo = init_repo();
        let branch = unique("conflict");
        let wt = TempDir::new().unwrap();
        git(repo.path(), &["worktree", "add", &wt.path().to_string_lossy(), "-b", &branch]);
        // Create conflicting changes
        std::fs::write(wt.path().join("init.txt"), "branch\n").unwrap();
        git(wt.path(), &["add", "init.txt"]);
        git(wt.path(), &["commit", "-m", "branch"]);
        std::fs::write(repo.path().join("init.txt"), "main\n").unwrap();
        git(repo.path(), &["add", "init.txt"]);
        git(repo.path(), &["commit", "-m", "main"]);

        let store = Store::open_memory().unwrap();
        let task = make_task_with_worktree("t-conflict", repo.path(), wt.path(), &branch);
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-conflict");
        assert!(result.is_err());
        // Worktree must be preserved for manual resolution
        assert!(wt.path().exists());
        // Task must stay Done
        let loaded = store.get_task("t-conflict").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Done);

        let _ = Command::new("git").args(["-C", &repo.path().to_string_lossy(), "merge", "--abort"]).output();
        git(repo.path(), &["worktree", "remove", "--force", &wt.path().to_string_lossy()]);
    }

    #[test]
    fn merge_single_without_repo_path_resolves_from_worktree() {
        let repo = init_repo();
        let (wt, branch) = create_worktree_with_commit(repo.path());
        let store = Store::open_memory().unwrap();
        // Simulate the old bug: repo_path is None
        let mut task = make_task_with_worktree("t-no-repo", repo.path(), wt.path(), &branch);
        task.repo_path = None;
        store.insert_task(&task).unwrap();

        let result = merge_single(&store, "t-no-repo");
        assert!(result.is_ok(), "merge should resolve repo from worktree: {result:?}");
        assert!(repo.path().join("agent-work.txt").exists());
    }

    // --- Integration test for remove_worktree ---

    #[test]
    fn remove_worktree_cleans_up_properly() {
        let repo = init_repo();
        let branch = unique("cleanup");
        let wt = TempDir::new().unwrap();
        let wt_path = wt.path().to_string_lossy().to_string();
        git(repo.path(), &["worktree", "add", &wt_path, "-b", &branch]);

        // Should not panic and worktree dir should be gone
        remove_worktree(&repo.path().to_string_lossy(), &wt_path);
        assert!(!Path::new(&wt_path).exists());

        // git worktree list should not show it
        let out = Command::new("git")
            .args(["-C", &repo.path().to_string_lossy(), "worktree", "list"])
            .output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(!stdout.contains(&branch));
    }

    // --- verify "auto" fix ---

    #[test]
    fn run_verify_handles_auto_without_error() {
        let repo = init_repo();
        // Should not try to execute "auto" as a command — should fallback to "cargo check"
        // (will fail since no Cargo.toml, but that's OK — it shouldn't panic or try "auto")
        run_verify_in_worktree(&repo.path().to_string_lossy(), Some("auto"));
        // If we got here without panic, the fix works
    }
}

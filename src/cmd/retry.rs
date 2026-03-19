// Handler for `aid retry` plus a silent helper that returns the new task id.
// Reuses the original task config and dispatches a child task with feedback.

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::TaskId;

pub struct RetryArgs {
    pub task_id: String,
    pub feedback: String,
    pub agent: Option<String>,
    pub dir: Option<String>,
    pub reset: bool,
}

pub async fn run(store: Arc<Store>, args: RetryArgs) -> Result<TaskId> {
    let retry_id = retry_task(store, args, true).await?;
    aid_hint!("[aid] Watch: aid watch --quiet {}", retry_id);
    aid_hint!("[aid] TUI:   aid watch --tui");
    Ok(retry_id)
}

pub async fn retry_task(store: Arc<Store>, args: RetryArgs, announce: bool) -> Result<TaskId> {
    let task = store
        .get_task(&args.task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", args.task_id))?;
    let prompt = format!(
        "[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{prompt}",
        feedback = args.feedback,
        prompt = task.prompt,
    );
    let worktree = reusable_worktree(&task);
    let (dir, worktree_arg) = if args.dir.is_some() {
        (args.dir, None) // --dir override takes precedence
    } else {
        resolve_retry_target(&task, worktree, &args.task_id, args.reset)?
    };

    if announce {
        println!(
            "Retrying {} with feedback: {}",
            task.id,
            truncate(&args.feedback, 60)
        );
    }

    let agent_name = args.agent.unwrap_or_else(|| task.agent_display_name().to_string());
    let session_id = if task.agent == crate::types::AgentKind::OpenCode {
        task.agent_session_id.clone()
    } else {
        None
    };
    run::run(
        store,
        RunArgs {
            agent_name,
            prompt,
            repo: task.repo_path.clone(),
            dir,
            output: task.output_path.clone(),
            model: task.model.clone(),
            worktree: worktree_arg,
            group: task.workgroup_id.clone(),
            verify: task.verify.clone(),
            announce,
            parent_task_id: Some(task.id.as_str().to_string()),
            read_only: task.read_only,
            budget: task.budget,
            session_id,
            ..Default::default()
        },
    )
    .await
}

fn reusable_worktree(task: &crate::types::Task) -> Option<String> {
    // Always return branch name if the original task used a worktree,
    // even if the worktree was auto-cleaned after failure.
    // The retry will reuse the existing worktree or recreate it.
    if task.worktree_path.is_some() {
        task.worktree_branch.clone()
    } else {
        None
    }
}

fn resolve_retry_target(
    task: &crate::types::Task,
    worktree: Option<String>,
    task_id: &str,
    reset: bool,
) -> Result<(Option<String>, Option<String>)> {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => {
            if reset {
                reset_dirty_worktree(path)?;
            } else {
                save_partial_work(path, task_id)?;
            }
            Ok((Some(path.clone()), None))
        }
        Some(_) => {
            // Worktree was cleaned up (e.g. auto-cleanup after failure) —
            // pass branch name so run::run recreates a fresh worktree
            Ok((None, worktree))
        }
        None => Ok((None, None)),
    }
}

fn save_partial_work(path: &str, task_id: &str) -> Result<()> {
    if worktree_is_dirty(path)? {
        run_git(path, &["add", "-A"])?;
        run_git(path, &["commit", "-m", &format!("[aid] partial work from {task_id}")])?;
        aid_info!("[aid] Saved partial work from prior attempt as commit");
    }
    Ok(())
}

fn reset_dirty_worktree(path: &str) -> Result<()> {
    if worktree_is_dirty(path)? {
        aid_info!("[aid] Worktree has uncommitted changes from prior attempt, resetting...");
        run_git(path, &["checkout", "."])?;
        run_git(path, &["clean", "-fd"])?;
    }
    Ok(())
}

fn worktree_is_dirty(path: &str) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["-C", path, "status", "--porcelain"])
        .output()?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

fn run_git(path: &str, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

#[cfg(test)]
mod tests {
    use super::{reset_dirty_worktree, save_partial_work};
    use crate::test_subprocess;
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn save_partial_work_commits_dirty_files() {
        let _permit = test_subprocess::acquire();
        let temp = tempfile::tempdir().unwrap();
        init_repo(temp.path());
        write_file(temp.path(), "tracked.txt", "base\n");
        git(temp.path(), &["add", "tracked.txt"]);
        git(temp.path(), &["commit", "-m", "initial"]);

        write_file(temp.path(), "tracked.txt", "changed\n");
        write_file(temp.path(), "new.txt", "new\n");

        save_partial_work(temp.path().to_str().unwrap(), "t-1234").unwrap();

        assert_eq!(head_message(temp.path()), "[aid] partial work from t-1234");
        assert!(git_stdout(temp.path(), &["status", "--porcelain"]).is_empty());
        assert_eq!(
            git_stdout(temp.path(), &["show", "--name-only", "--format=", "HEAD"]),
            "new.txt\ntracked.txt\n"
        );
    }

    #[test]
    fn reset_dirty_worktree_discards_dirty_files() {
        let _permit = test_subprocess::acquire();
        let temp = tempfile::tempdir().unwrap();
        init_repo(temp.path());
        write_file(temp.path(), "tracked.txt", "base\n");
        git(temp.path(), &["add", "tracked.txt"]);
        git(temp.path(), &["commit", "-m", "initial"]);

        write_file(temp.path(), "tracked.txt", "changed\n");
        write_file(temp.path(), "new.txt", "new\n");

        reset_dirty_worktree(temp.path().to_str().unwrap()).unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.path().join("tracked.txt")).unwrap(),
            "base\n"
        );
        assert!(!temp.path().join("new.txt").exists());
        assert!(git_stdout(temp.path(), &["status", "--porcelain"]).is_empty());
        assert_eq!(head_message(temp.path()), "initial");
    }

    #[test]
    fn clean_worktree_is_not_modified() {
        let _permit = test_subprocess::acquire();
        let temp = tempfile::tempdir().unwrap();
        init_repo(temp.path());
        write_file(temp.path(), "tracked.txt", "base\n");
        git(temp.path(), &["add", "tracked.txt"]);
        git(temp.path(), &["commit", "-m", "initial"]);

        save_partial_work(temp.path().to_str().unwrap(), "t-1234").unwrap();

        assert_eq!(head_message(temp.path()), "initial");
        assert!(git_stdout(temp.path(), &["status", "--porcelain"]).is_empty());
    }

    fn init_repo(path: &Path) {
        git(path, &["init"]);
        git(path, &["config", "user.name", "Test User"]);
        git(path, &["config", "user.email", "test@example.com"]);
    }

    fn write_file(path: &Path, name: &str, contents: &str) {
        std::fs::write(path.join(name), contents).unwrap();
    }

    fn head_message(path: &Path) -> String {
        git_stdout(path, &["log", "-1", "--pretty=%s"]).trim().to_string()
    }

    fn git_stdout(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
}

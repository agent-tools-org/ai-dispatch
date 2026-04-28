// `aid doctor` reports prunable worktrees and merged aid branches.
// Exports run() plus formatting helpers shared by tests.
// Deps: crate::repo_root, crate::store::Store, crate::worktree_gc.

use crate::project;
use crate::repo_root;
use crate::store::Store;
use crate::worktree_gc::{
    BranchDeleteOutcome, DeletableBranch, DoctorReport, PrunableWorktree, collect_doctor_report,
    delete_local_branch, managed_branch_prefixes, tracked_worktree_paths,
};
use anyhow::Result;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

pub fn run(store: &Arc<Store>, apply: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo_dir = repo_root::resolve_git_root_string(&cwd.to_string_lossy())?;
    let repo_dir = Path::new(&repo_dir);
    let tracked_paths = tracked_worktree_paths(store.as_ref())?;
    let prefixes = managed_branch_prefixes(project::detect_project().as_ref());
    let report = collect_doctor_report(repo_dir, &tracked_paths, &prefixes)?;
    print!("{}", format_report(&report));
    if !apply {
        return Ok(());
    }

    if !report.prunable_worktrees.is_empty() {
        let status = Command::new("git")
            .args(["-C", &repo_dir.to_string_lossy(), "worktree", "prune"])
            .status()?;
        anyhow::ensure!(status.success(), "git worktree prune failed");
    }
    for branch in &report.deletable_branches {
        match delete_local_branch(repo_dir, &branch.branch)? {
            BranchDeleteOutcome::Deleted => {
                println!("applied branch delete: {}", branch.branch);
            }
            BranchDeleteOutcome::Missing => {
                println!("branch already gone: {}", branch.branch);
            }
            BranchDeleteOutcome::Kept(note) => {
                println!("branch kept: {} ({note})", branch.branch);
            }
        }
    }
    Ok(())
}

pub(crate) fn format_report(report: &DoctorReport) -> String {
    let mut rendered = String::new();
    render_prunable_section(&mut rendered, &report.prunable_worktrees);
    rendered.push('\n');
    render_branch_section(
        &mut rendered,
        &report.base_branch,
        &report.deletable_branches,
    );
    rendered
}

fn render_prunable_section(rendered: &mut String, worktrees: &[PrunableWorktree]) {
    let _ = writeln!(rendered, "Prunable worktrees ({})", worktrees.len());
    let _ = writeln!(rendered, "{:<60}", "PATH");
    let _ = writeln!(rendered, "{}", "-".repeat(60));
    if worktrees.is_empty() {
        let _ = writeln!(rendered, "(none)");
        return;
    }
    for item in worktrees {
        let _ = writeln!(rendered, "{:<60}", item.path);
    }
}

fn render_branch_section(
    rendered: &mut String,
    base_branch: &str,
    branches: &[DeletableBranch],
) {
    let _ = writeln!(
        rendered,
        "Deletable branches ({}) against {}",
        branches.len(),
        base_branch
    );
    let _ = writeln!(rendered, "{:<36} REASON", "BRANCH");
    let _ = writeln!(rendered, "{}", "-".repeat(72));
    if branches.is_empty() {
        let _ = writeln!(rendered, "(none)");
        return;
    }
    for item in branches {
        let _ = writeln!(
            rendered,
            "{:<36} {}",
            item.branch,
            item.reason.label()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::format_report;
    use crate::worktree_gc::{DeletableBranch, DoctorReport, MergeReason, PrunableWorktree};

    #[test]
    fn format_report_renders_two_sections() {
        let report = DoctorReport {
            base_branch: "main".to_string(),
            prunable_worktrees: vec![PrunableWorktree {
                path: "/Users/test/.aid/worktrees/demo/feat/old".to_string(),
            }],
            deletable_branches: vec![DeletableBranch {
                branch: "feat/merged".to_string(),
                reason: MergeReason::CherryEmpty,
            }],
        };

        let rendered = format_report(&report);

        assert!(rendered.contains("Prunable worktrees (1)"));
        assert!(rendered.contains("/Users/test/.aid/worktrees/demo/feat/old"));
        assert!(rendered.contains("Deletable branches (1) against main"));
        assert!(rendered.contains("feat/merged"));
        assert!(rendered.contains("merged (git cherry empty)"));
    }
}

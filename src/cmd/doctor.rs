// `aid doctor` reports repository hygiene and leaked operator symlinks.
// Exports run() plus formatting helpers shared by tests.
// Deps: crate::repo_root, crate::store::Store, crate::worktree_gc.

use crate::project;
use crate::repo_root;
use crate::store::Store;
use crate::{agent::home_isolation, paths};
use crate::worktree_gc::{
    DeletableBranch, DoctorReport, PrunableWorktree, collect_doctor_report,
    managed_branch_prefixes, tracked_worktree_paths,
};
use anyhow::Result;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

pub fn run(store: &Arc<Store>, apply: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo_dir = repo_root::resolve_git_root_string(&cwd.to_string_lossy())?;
    let repo_dir = Path::new(&repo_dir);
    let tracked_paths = tracked_worktree_paths(store.as_ref())?;
    let prefixes = managed_branch_prefixes(project::detect_project().as_ref());
    let report = collect_doctor_report(repo_dir, &tracked_paths, &prefixes)?;
    let real_home = home_isolation::resolve_real_home()?;
    let symlink_repairs = home_isolation::find_doctor_symlinks(&real_home, &paths::aid_dir())?;
    print!("{}", format_report(&report));
    print!("{}", format_symlink_report(&symlink_repairs));
    if !apply {
        return Ok(());
    }

    home_isolation::apply_repairs(&symlink_repairs)?;
    if !symlink_repairs.is_empty() {
        println!("Repaired {} leaked operator symlink(s)", symlink_repairs.len());
    }

    if !report.prunable_worktrees.is_empty() || !report.deletable_branches.is_empty() {
        anyhow::bail!(
            "Doctor will not delete task artifacts; use explicit principal acceptance followed by `aid gc --task <task>`"
        );
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

pub(crate) fn format_symlink_report(repairs: &[home_isolation::SymlinkRepair]) -> String {
    let mut rendered = String::new();
    let _ = writeln!(rendered, "Leaked operator symlinks ({})", repairs.len());
    let _ = writeln!(rendered, "LINK -> REWRITE");
    let _ = writeln!(rendered, "{}", "-".repeat(72));
    if repairs.is_empty() {
        let _ = writeln!(rendered, "(none)");
        return rendered;
    }
    for repair in repairs {
        let _ = writeln!(
            rendered,
            "{} -> {}",
            repair.link_path.display(),
            repair.rewritten_target.display()
        );
    }
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
    use super::{format_report, format_symlink_report};
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

    #[cfg(unix)]
    #[test]
    fn doctor_repairs_deleted_task_and_tmp_home_symlink_targets() {
        let fixture = tempfile::tempdir().expect("fixture");
        let real_home = fixture.path().join("real-home");
        let aid_dir = fixture.path().join(".aid");
        let bin = real_home.join(".local/bin");
        std::fs::create_dir_all(&bin).expect("bin");

        let task_link = bin.join("task-tool");
        let task_target = aid_dir.join("tasks/t-deleted/home/.local/bin/task-tool");
        std::os::unix::fs::symlink(&task_target, &task_link).expect("task link");
        let tmp_link = bin.join("tmp-tool");
        let tmp_target = aid_dir.join("tmp_home/iso-deleted/home/.local/bin/tmp-tool");
        std::os::unix::fs::symlink(&tmp_target, &tmp_link).expect("tmp link");

        let repairs = crate::agent::home_isolation::find_doctor_symlinks(&real_home, &aid_dir)
            .expect("scan");
        assert_eq!(repairs.len(), 2);
        let rendered = format_symlink_report(&repairs);
        assert!(rendered.contains(&task_link.display().to_string()));
        assert!(rendered.contains(&real_home.join(".local/bin/task-tool").display().to_string()));
        assert!(rendered.contains(&tmp_link.display().to_string()));

        crate::agent::home_isolation::apply_repairs(&repairs).expect("apply");
        assert_eq!(std::fs::read_link(task_link).expect("task rewrite"), real_home.join(".local/bin/task-tool"));
        assert_eq!(std::fs::read_link(tmp_link).expect("tmp rewrite"), real_home.join(".local/bin/tmp-tool"));
    }
}

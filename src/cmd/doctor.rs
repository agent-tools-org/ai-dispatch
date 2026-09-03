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
    print!("{}", format_report(&report));
    let symlink_scan = match home_isolation::resolve_real_home() {
        Ok(real_home) => home_isolation::scan_doctor_symlinks(&real_home, &paths::aid_dir()),
        Err(err) => {
            aid_warn!("[aid] Warning: cannot scan leaked operator symlinks: {err:#}");
            home_isolation::SymlinkScan { repairs: Vec::new(), complete: false }
        }
    };
    if !symlink_scan.complete {
        aid_warn!("[aid] Warning: leaked operator symlink scan was incomplete");
    }
    print!("{}", format_symlink_report(&symlink_scan.repairs));
    if !apply {
        return Ok(());
    }

    let repaired = home_isolation::apply_repairs(&symlink_scan.repairs)?;
    if repaired > 0 {
        println!("Repaired {repaired} leaked operator symlink(s)");
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
        let status = if home_isolation::is_repairable(repair) {
            ""
        } else {
            " (unrepairable)"
        };
        let _ = writeln!(rendered, "{} -> {}{status}", repair.link_path.display(), repair.rewritten_target.display());
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
        let task_payload = real_home.join(".local/share/task-tool/v1/task-tool");
        let tmp_payload = real_home.join(".local/share/tmp-tool/v1/tmp-tool");
        std::fs::create_dir_all(task_payload.parent().expect("task payload parent")).expect("task payload dirs");
        std::fs::create_dir_all(tmp_payload.parent().expect("tmp payload parent")).expect("tmp payload dirs");
        std::fs::write(&task_payload, "task payload").expect("task payload");
        std::fs::write(&tmp_payload, "tmp payload").expect("tmp payload");

        let task_link = bin.join("task-tool");
        let task_target = aid_dir.join("tasks/t-deleted/home/.local/share/task-tool/v1/task-tool");
        std::os::unix::fs::symlink(&task_target, &task_link).expect("task link");
        let tmp_link = bin.join("tmp-tool");
        let tmp_target = aid_dir.join("tmp_home/iso-deleted/home/.local/share/tmp-tool/v1/tmp-tool");
        std::os::unix::fs::symlink(&tmp_target, &tmp_link).expect("tmp link");
        let missing_link = bin.join("missing-tool");
        let missing_target = aid_dir.join("tasks/t-deleted/home/.local/share/missing-tool/v1/missing-tool");
        std::os::unix::fs::symlink(&missing_target, &missing_link).expect("missing link");
        let missing_target_before = std::fs::read_link(&missing_link).expect("missing target");

        let repairs = crate::agent::home_isolation::find_doctor_symlinks(&real_home, &aid_dir)
            .expect("scan");
        assert_eq!(repairs.len(), 3);
        let rendered = format_symlink_report(&repairs);
        assert!(rendered.contains(&task_link.display().to_string()));
        assert!(rendered.contains(&task_payload.display().to_string()));
        assert!(rendered.contains(&tmp_link.display().to_string()));
        assert!(rendered.contains(&missing_link.display().to_string()));
        assert!(rendered.contains("missing-tool") && rendered.contains("unrepairable"));

        let repaired = crate::agent::home_isolation::apply_repairs(&repairs).expect("apply");
        assert_eq!(repaired, 2);
        assert_eq!(std::fs::read_to_string(task_link).expect("task resolves"), "task payload");
        assert_eq!(std::fs::read_to_string(tmp_link).expect("tmp resolves"), "tmp payload");
        assert_eq!(std::fs::read_link(missing_link).expect("missing survives"), missing_target_before);
    }
}

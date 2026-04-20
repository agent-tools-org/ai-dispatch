// Handler for `aid group` commands.
// Creates, lists, and shows workgroups plus their shared context and member tasks.
// Depends on Store for persistence and board-style task listing.

use anyhow::Result;
use std::sync::Arc;

use crate::sanitize;
use crate::store::Store;
use crate::types::TaskFilter;

pub fn create(
    store: &Arc<Store>,
    name: &str,
    context: &str,
    custom_id: Option<&str>,
) -> Result<()> {
    if let Some(custom_id) = custom_id {
        sanitize::validate_workgroup_id(custom_id)?;
    }
    let workgroup = store.create_workgroup(name, context, Some("cli"), custom_id)?;
    println!("{}", workgroup.id);
    aid_info!(
        "[aid] Created workgroup '{}' ({})",
        workgroup.name,
        workgroup.id
    );
    aid_hint!(
        "[aid] Scope all commands: export AID_GROUP={}",
        workgroup.id
    );
    aid_info!(
        "[aid] Workspace: {}",
        crate::paths::workspace_dir(workgroup.id.as_str())?.display()
    );
    Ok(())
}

pub fn list(store: &Arc<Store>) -> Result<()> {
    let workgroups = store.list_workgroups()?;
    if workgroups.is_empty() {
        println!("No workgroups found.");
        return Ok(());
    }

    let tasks = store.list_tasks(TaskFilter::All)?;
    println!("{:<10} {:<20} {:<8} Updated", "ID", "Name", "Tasks");
    println!("{}", "-".repeat(56));
    for workgroup in workgroups {
        let task_count = tasks
            .iter()
            .filter(|task| task.workgroup_id.as_deref() == Some(workgroup.id.as_str()))
            .count();
        println!(
            "{:<10} {:<20} {:<8} {}",
            workgroup.id,
            truncate(&workgroup.name, 20),
            task_count,
            workgroup.updated_at.format("%Y-%m-%d %H:%M")
        );
    }
    Ok(())
}

pub fn show(store: &Arc<Store>, workgroup_id: &str) -> Result<()> {
    let workgroup = store
        .get_workgroup(workgroup_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", workgroup_id))?;
    let tasks = store
        .list_tasks(TaskFilter::All)?
        .into_iter()
        .filter(|task| task.workgroup_id.as_deref() == Some(workgroup.id.as_str()))
        .collect::<Vec<_>>();

    println!("Workgroup: {} ({})", workgroup.id, workgroup.name);
    println!(
        "Updated: {}",
        workgroup.updated_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!("\nShared context:\n{}", workgroup.shared_context);
    println!("\nTasks:");
    if tasks.is_empty() {
        println!("  (none)");
    } else {
        for task in tasks {
            println!(
                "  {}  {:<8}  {}",
                task.id,
                task.status.label(),
                truncate(&task.prompt, 60)
            );
        }
    }
    Ok(())
}

pub fn update(
    store: &Arc<Store>,
    workgroup_id: &str,
    name: Option<&str>,
    context: Option<&str>,
) -> Result<()> {
    if name.is_none() && context.is_none() {
        anyhow::bail!("Provide --name and/or --context");
    }

    let workgroup = store
        .update_workgroup(workgroup_id, name, context)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", workgroup_id))?;
    println!("Workgroup {} updated", workgroup.id);
    println!("Name: {}", workgroup.name);
    println!("Shared context:\n{}", workgroup.shared_context);
    Ok(())
}

pub fn delete(store: &Arc<Store>, workgroup_id: &str, cascade: bool) -> Result<()> {
    if cascade {
        let deleted_tasks = store
            .delete_workgroup_cascade(workgroup_id)?
            .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", workgroup_id))?;
        println!("Deleted {} tasks and group {}", deleted_tasks, workgroup_id);
        return Ok(());
    }

    let tagged_tasks = store
        .delete_workgroup(workgroup_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", workgroup_id))?;
    println!("Workgroup {} deleted", workgroup_id);
    println!(
        "Historical tasks still tagged: {} — use --cascade to also delete them",
        tagged_tasks
    );
    Ok(())
}

pub fn cancel(store: &Arc<Store>, workgroup_id: &str) -> Result<()> {
    store
        .get_workgroup(workgroup_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", workgroup_id))?;
    let mut cancelled = 0;
    for task in store.list_tasks_by_group(workgroup_id)? {
        if task.status.is_terminal() {
            continue;
        }
        crate::cmd::stop::terminate_any(store, task.id.as_str())?;
        cancelled += 1;
    }
    println!("Cancelled {cancelled} tasks in group {workgroup_id}");
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let safe = value.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &value[..safe])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;
    use tempfile::TempDir;

    fn make_task(id: &str, group_id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status,
            parent_task_id: None,
            workgroup_id: Some(group_id.to_string()),
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn cancel_group_stops_only_non_terminal_tasks() {
        let temp = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        let store = Arc::new(Store::open_memory().unwrap());
        store.create_workgroup("demo", "", Some("cli"), Some("wg-1")).unwrap();
        for task in [
            make_task("t-wait", "wg-1", TaskStatus::Waiting),
            make_task("t-pend", "wg-1", TaskStatus::Pending),
            make_task("t-run", "wg-1", TaskStatus::Running),
            make_task("t-done", "wg-1", TaskStatus::Done),
            make_task("t-other", "wg-2", TaskStatus::Running),
        ] {
            store.insert_task(&task).unwrap();
        }

        cancel(&store, "wg-1").unwrap();

        assert_eq!(store.get_task("t-wait").unwrap().unwrap().status, TaskStatus::Stopped);
        assert_eq!(store.get_task("t-pend").unwrap().unwrap().status, TaskStatus::Stopped);
        assert_eq!(store.get_task("t-run").unwrap().unwrap().status, TaskStatus::Stopped);
        assert_eq!(store.get_task("t-done").unwrap().unwrap().status, TaskStatus::Done);
        assert_eq!(store.get_task("t-other").unwrap().unwrap().status, TaskStatus::Running);
    }

    #[test]
    fn delete_group_without_cascade_keeps_tagged_tasks() {
        let temp = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        let store = Arc::new(Store::open_memory().unwrap());
        store.create_workgroup("demo", "", Some("cli"), Some("wg-1")).unwrap();
        store
            .insert_task(&make_task("t-keep", "wg-1", TaskStatus::Done))
            .unwrap();

        delete(&store, "wg-1", false).unwrap();

        assert!(store.get_workgroup("wg-1").unwrap().is_none());
        assert!(store.get_task("t-keep").unwrap().is_some());
    }

    #[test]
    fn delete_group_with_cascade_removes_group_and_tasks() {
        let temp = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        let store = Arc::new(Store::open_memory().unwrap());
        store.create_workgroup("demo", "", Some("cli"), Some("wg-1")).unwrap();
        store
            .insert_task(&make_task("t-drop", "wg-1", TaskStatus::Done))
            .unwrap();

        delete(&store, "wg-1", true).unwrap();

        assert!(store.get_workgroup("wg-1").unwrap().is_none());
        assert!(store.get_task("t-drop").unwrap().is_none());
    }
}

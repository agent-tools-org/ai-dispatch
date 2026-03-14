// Handler for `aid group` commands.
// Creates, lists, and shows workgroups plus their shared context and member tasks.
// Depends on Store for persistence and board-style task listing.

use anyhow::Result;
use std::sync::Arc;

use crate::store::Store;
use crate::types::TaskFilter;

pub fn create(store: &Arc<Store>, name: &str, context: &str) -> Result<()> {
    let workgroup = store.create_workgroup(name, context)?;
    println!("{}", workgroup.id);
    eprintln!("[aid] Created workgroup '{}' ({})", workgroup.name, workgroup.id);
    eprintln!("[aid] Scope all commands: export AID_GROUP={}", workgroup.id);
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
    println!("Updated: {}", workgroup.updated_at.format("%Y-%m-%d %H:%M:%S"));
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

pub fn delete(store: &Arc<Store>, workgroup_id: &str) -> Result<()> {
    let tagged_tasks = store
        .delete_workgroup(workgroup_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", workgroup_id))?;
    println!("Workgroup {} deleted", workgroup_id);
    println!("Historical tasks still tagged: {}", tagged_tasks);
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max.saturating_sub(3)])
    }
}

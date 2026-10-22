// Display helpers for the memory command.
// Exports: command phases used by the parent module.
// Deps: parent command types and supporting configuration.

use crate::types::Memory;
use crate::ui::format::truncate;
use chrono::{DateTime, Local};
pub(super) fn print_memory_table(memories: &[Memory], stats: bool) {
    if memories.is_empty() {
        println!("No memories found.");
        return;
    }
    if stats {
        println!(
            "{:<10} {:<10} {:<8} {:<9} {:<19} CONTENT",
            "ID", "TYPE", "INJECTS", "SUCCESS", "LAST USED"
        );
        println!("{}", "-".repeat(120));
        for memory in memories {
            println!(
                "{:<10} {:<10} {:<8} {:<9} {:<19} {}",
                memory.id,
                memory.memory_type.label(),
                memory.inject_count,
                memory.success_count,
                format_last_used(memory.last_injected_at.as_ref()),
                truncate(&memory.content, 60),
            );
        }
    } else {
        println!(
            "{:<10} {:<10} {:<60} {:<8} SOURCE",
            "ID", "TYPE", "CONTENT", "AGE"
        );
        println!("{}", "-".repeat(94));
        for memory in memories {
            println!(
                "{:<10} {:<10} {:<60} {:<8} {}",
                memory.id,
                memory.memory_type.label(),
                truncate(&memory.content, 60),
                format_age(&memory.created_at),
                memory.project_path.as_deref().unwrap_or("-"),
            );
        }
    }
}

fn format_last_used(last: Option<&DateTime<Local>>) -> String {
    last.map(format_age).unwrap_or_else(|| "-".to_string())
}

pub(super) fn format_age(created_at: &DateTime<Local>) -> String {
    let now = Local::now();
    let duration = now.signed_duration_since(*created_at);
    let mins = duration.num_minutes();
    if mins <= 0 {
        format!("{}s", duration.num_seconds().max(0))
    } else if mins < 60 {
        format!("{}m", mins)
    } else if mins < 24 * 60 {
        format!("{}h", duration.num_hours())
    } else {
        format!("{}d", duration.num_days())
    }
}

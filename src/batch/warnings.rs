// Batch warning helpers for prompt size and read-only audit hints.
// Exports: warn_prompt_size(), warn_audit_without_readonly().
// Deps: std collections/io and parent `BatchTask`.

use std::io::{self, Write};

use super::BatchTask;

pub(super) fn warn_prompt_size(tasks: &[BatchTask], writer: &mut impl Write) -> io::Result<()> {
    for (idx, task) in tasks.iter().enumerate() {
        let chars = task.prompt.len();
        if chars > 6000 {
            writeln!(
                writer,
                "[aid] Warning: task '{}' has a large prompt (~{} chars, {} lines). Consider splitting into smaller tasks for better agent execution quality.",
                task_label(task, idx),
                chars,
                task.prompt.lines().count(),
            )?;
        }
    }
    Ok(())
}

pub fn warn_audit_without_readonly(tasks: &[BatchTask]) {
    let _ = warn_audit_without_readonly_into(tasks, &mut io::stderr().lock());
}

pub(crate) fn warn_audit_without_readonly_into(
    tasks: &[BatchTask],
    writer: &mut impl Write,
) -> io::Result<()> {
    for (task_idx, task) in tasks.iter().enumerate() {
        if task.read_only || !prompt_suggests_read_only(&task.prompt) {
            continue;
        }
        writeln!(
            writer,
            "[aid] ⚠ Task '{}' prompt suggests read-only intent but read_only is not set. Consider adding read_only = true",
            task_label(task, task_idx)
        )?;
    }
    Ok(())
}

fn prompt_suggests_read_only(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    lower.contains("do not modify")
        || lower.contains("don't modify")
        || lower.contains("report only")
        || lower.contains("read only")
        || lower.contains("read-only")
        || lower.contains("do not change")
        || lower.contains("analysis only")
        || lower.contains("analyze only")
        || (lower.contains("audit")
            && !lower.contains("audit trail")
            && !lower.contains("audit log"))
}

fn task_label(task: &BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}

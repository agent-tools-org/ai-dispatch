// Handler for `aid board` — list all tasks with status summary.
// Detects repeated calls with no status changes and warns callers.
// Deps: crate::store, crate::board, crate::background, crate::session

use anyhow::Result;
use chrono::Local;
use std::sync::Arc;

use crate::background;
use crate::board::render_board;
use crate::session;
use crate::store::Store;
use crate::types::TaskFilter;

pub fn run(
    store: &Arc<Store>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
    json: bool,
) -> Result<()> {
    let filter = if running {
        TaskFilter::Running
    } else if today {
        TaskFilter::Today
    } else {
        TaskFilter::All
    };

    background::check_zombie_tasks(store)?;
    let mut tasks = store.list_tasks(filter)?;
    if mine {
        tasks.retain(session::matches_current);
    }
    if let Some(group_id) = group {
        tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    }

    // Detect repeated calls with no changes — warn or reject rapid polling
    let fingerprint = task_fingerprint(&tasks);
    let marker_path = crate::paths::aid_dir().join("board-last.txt");
    if let Ok(prev) = std::fs::read_to_string(&marker_path) {
        // Format: "timestamp\nfingerprint"
        let mut lines = prev.splitn(2, '\n');
        let prev_ts: i64 = lines
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let prev_fp = lines.next().unwrap_or("");
        if prev_fp == fingerprint {
            let now_ts = Local::now().timestamp();
            if now_ts - prev_ts < 10 {
                // Rapid repeated call with no changes — hard error to break polling loops
                eprintln!("[aid] ERROR: No changes detected. Stop polling — use `aid watch --quiet --group <wg-id>` instead.");
                eprintln!("[aid] Board calls within 10s of identical state are rejected.");
                std::process::exit(1);
            } else {
                eprintln!("[aid] No status changes since last check. Use `aid watch --quiet` for automatic notification instead of polling.");
            }
        }
    }
    let content = format!("{}\n{}", Local::now().timestamp(), fingerprint);
    let _ = std::fs::write(&marker_path, &content);

    if json {
        let payload: Vec<serde_json::Value> = tasks
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id.as_str(),
                    "agent": t.agent_display_name(),
                    "status": t.status.as_str(),
                    "prompt": t.prompt,
                    "model": t.model,
                    "tokens": t.tokens,
                    "duration_ms": t.duration_ms,
                    "cost_usd": t.cost_usd,
                    "workgroup_id": t.workgroup_id,
                    "worktree_branch": t.worktree_branch,
                    "verify_status": t.verify_status.as_str(),
                    "created_at": t.created_at.to_rfc3339(),
                    "completed_at": t.completed_at.map(|dt| dt.to_rfc3339()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }
    print!("{}", render_board(&tasks, store)?);
    Ok(())
}

/// Compact fingerprint of task statuses for change detection.
fn task_fingerprint(tasks: &[crate::types::Task]) -> String {
    let mut parts: Vec<String> = tasks
        .iter()
        .map(|t| format!("{}:{}", t.id, t.status.label()))
        .collect();
    parts.sort();
    parts.join(",")
}

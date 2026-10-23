// Status transition and final outcome reporting for aid wait.
// Deps: parent wait outcome, Store, task types, and cost formatting.

use super::*;

pub(super) fn print_task_transition(
    store: &Store,
    task_id: &str,
    task: &Task,
    completed: &mut usize,
    total: usize,
) {
    let status = task.status;
    let outcome = task.outcome();
    if status.is_terminal() {
        *completed += 1;
        let duration = task
            .duration_ms
            .map(crate::ui::format::format_duration_ms)
            .unwrap_or_else(|| "-".to_string());
        let tokens = task
            .tokens
            .map(crate::ui::format::format_tokens)
            .unwrap_or_else(|| "-".to_string());
        let fail_reason = if status == TaskStatus::Failed {
            store
                .latest_error(task_id)
                .map(|r| format!(" — {r}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let status_label = outcome
            .verification_tag()
            .map(|tag| format!("{} [{tag}]", status.label()))
            .unwrap_or_else(|| status.label().to_string());
        println!(
            "[{}/{}] {} {} ({}, {}tok, {}){fail_reason}",
            completed,
            total,
            task_id,
            status_label,
            duration,
            tokens,
            cost::format_cost(task.cost_usd),
        );
    } else {
        println!("[{}/{}] {} {}", completed, total, task_id, status.label());
    }
}

pub(super) fn print_waiting_prompt(store: &Store, task_id: &str) -> Result<()> {
    let prompt = store.latest_waiting_prompt(task_id)?.unwrap_or_default();
    println!("{} {}", task_id, prompt);
    println!("Use: aid respond {} \"your answer\"", task_id);
    Ok(())
}

pub(super) fn report_wait_outcome(
    outcome: WaitOutcome,
    tracked_group: Option<&str>,
    timeout_secs: Option<u64>,
) -> Result<()> {
    match outcome {
        WaitOutcome::Completed => {
            if let Some(group_id) = tracked_group
                && let Ok(repo_path) = crate::support::repo_root::resolve_git_root_string(".")
                && let Some(hint) = crate::cmd::batch_gitbutler::merge_back_hint(
                    std::path::Path::new(&repo_path),
                    group_id,
                )
            {
                println!("{hint}");
            }
            Ok(())
        }
        WaitOutcome::Failed(task_ids) => {
            aid_error!(
                "[aid] {} task(s) did not succeed: {}",
                task_ids.len(),
                task_ids.join(", ")
            );
            std::process::exit(1);
        }
        WaitOutcome::TimedOut(running) => {
            let secs = timeout_secs.unwrap_or_default();
            aid_error!(
                "[aid] Timeout after {}s. Still running: {}",
                secs,
                running.join(", ")
            );
            std::process::exit(124);
        }
    }
}

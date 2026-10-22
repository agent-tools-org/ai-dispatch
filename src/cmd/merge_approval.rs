// Approval workflow helpers for aid merge.
// Deps: parent merge guards, git helpers, Store, and task types.

use super::*;

pub(super) enum ApprovalDecision {
    Merge,
    Retry,
    Skip,
}

pub(super) fn ask_approval(task: &Task) -> Result<ApprovalDecision> {
    let branch = merge_source_branch(task).unwrap_or("-");
    let prompt = format!(
        "Task {} ready to merge:\n- Route: {}\n- Branch: {}\n\nApprove?",
        task.id,
        task.display_route(),
        branch
    );
    run_approval_prompt(
        &format!("Merge:aid merge {}", task.id),
        &format!("Retry:aid retry {}", task.id),
        &prompt,
    )
}

pub(super) fn ask_group_approval(group_id: &str, tasks: &[Task]) -> Result<ApprovalDecision> {
    let details = tasks
        .iter()
        .map(|task| {
            format!(
                "- {}: {} ({})",
                task.id,
                task.display_route(),
                merge_source_branch(task).unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!("Group {group_id} ready to merge:\n{details}\n\nApprove?");
    run_approval_prompt(
        &format!("Merge:aid merge --group {group_id}"),
        "Retry",
        &prompt,
    )
}

fn run_approval_prompt(
    merge_action: &str,
    retry_action: &str,
    prompt: &str,
) -> Result<ApprovalDecision> {
    let actions = format!("{merge_action},{retry_action},Skip");
    let cmd = get_hiboss_command();
    let output = match Command::new(&cmd)
        .args(["ask", "--actions", &actions, "--timeout", "300", prompt])
        .stdout(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ApprovalDecision::Merge);
        }
        Err(err) => return Err(err.into()),
    };
    let reply = String::from_utf8_lossy(&output.stdout);
    if reply.contains("Skip") {
        return Ok(ApprovalDecision::Skip);
    }
    if reply.contains("Retry") {
        return Ok(ApprovalDecision::Retry);
    }
    Ok(ApprovalDecision::Merge)
}

#[cfg(test)]
thread_local! {
    static TEST_HIBOSS_COMMAND: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_hiboss_command(cmd: Option<String>) {
    TEST_HIBOSS_COMMAND.with(|cell| *cell.borrow_mut() = cmd);
}

fn get_hiboss_command() -> String {
    #[cfg(test)]
    {
        if let Some(cmd) = TEST_HIBOSS_COMMAND.with(|cell| cell.borrow().clone()) {
            return cmd;
        }
    }
    "hiboss".to_string()
}

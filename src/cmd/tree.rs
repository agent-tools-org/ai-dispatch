// Handler for `aid tree <task-id>` — render retry chain as ASCII tree.
// Exports: `run(&Store, &str)`; relies on store queries and task metadata.
// Deps: crate::store::Store and crate::types::{Task, TaskFilter}.
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use crate::store::Store;
use crate::types::{Task, TaskFilter};
use crate::ui::format::format_tokens;
use crate::ui::format::format_duration_optional_ms;

pub fn run(store: &Store, task_id: &str) -> Result<()> {
    let chain = store.get_retry_chain(task_id)?;
    if chain.is_empty() {
        anyhow::bail!("Task '{task_id}' not found");
    }
    let chain_ids: HashSet<&str> = chain.iter().map(|task| task.id.as_str()).collect();
    let mut children_by_parent: HashMap<String, Vec<Task>> = HashMap::new();
    for task in store.list_tasks(TaskFilter::All)? {
        if let Some(parent_id) = task.parent_task_id.as_deref()
            && chain_ids.contains(parent_id)
        {
            children_by_parent.entry(parent_id.to_string()).or_default().push(task);
        }
    }
    let mut chain_child_idx: HashMap<String, usize> = HashMap::new();
    for (i, task) in chain.iter().enumerate().take(chain.len().saturating_sub(1)) {
        chain_child_idx.insert(task.id.as_str().to_string(), i + 1);
    }
    println!("{}", format_task_line(&chain[0]));
    render_children(
        &chain,
        0,
        "",
        &children_by_parent,
        &chain_child_idx,
    );
    Ok(())
}
fn render_children(
    chain: &[Task],
    parent_idx: usize,
    indent: &str,
    children_by_parent: &HashMap<String, Vec<Task>>,
    chain_child_idx: &HashMap<String, usize>,
) {
    let parent_id = chain[parent_idx].id.as_str();
    let chain_child_id = chain_child_idx
        .get(parent_id)
        .map(|&idx| &chain[idx].id);
    let mut entries: Vec<(Task, Option<usize>)> = Vec::new();
    if let Some(children) = children_by_parent.get(parent_id) {
        for child in children {
            let is_chain_child = chain_child_id == Some(&child.id);
            if !is_chain_child {
                entries.push((child.clone(), None));
            }
        }
    }
    if let Some(&child_idx) = chain_child_idx.get(parent_id) {
        entries.push((chain[child_idx].clone(), Some(child_idx)));
    }
    if entries.is_empty() {
        return;
    }
    let total = entries.len();
    for (i, (task, chain_idx)) in entries.into_iter().enumerate() {
        let is_last = i + 1 == total;
        let connector = if is_last { "└──" } else { "├──" };
        println!("{}{} {}", indent, connector, format_task_line(&task));
        if let Some(next_idx) = chain_idx {
            let child_indent = format!("{}{}", indent, if is_last { "    " } else { "│   " });
            render_children(
                chain,
                next_idx,
                &child_indent,
                children_by_parent,
                chain_child_idx,
            );
        }
    }
}
fn format_task_line(task: &Task) -> String {
    let mut status = task.status.label().to_string();
    if task.has_verify_failure() {
        status.push_str(" [verify:failed]");
    }
    if let Some(delivery) = task.delivery_assessment() {
        status.push_str(&format!(" [delivery:{}]", delivery.as_str()));
    }
    let duration = format_duration_optional_ms(task.duration_ms);
    let mut parts = vec![duration];
    if let Some(tokens) = task.tokens {
        parts.push(format!("{} tokens", format_tokens(tokens)));
    }
    format!(
        "{} {} {} ({})",
        task.id.as_str(),
        task.display_route(),
        status,
        parts.join(", ")
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AgentKind, AttributionSource, TaskId, TaskStatus, };


    fn sample(agent: AgentKind) -> Task {
        Task {
            agent,
            prompt: "p".to_string(),
            tokens: Some(1_500),
            duration_ms: Some(12_000),
            ..crate::types::Task::test_fixture(TaskId("t-tree".to_string()), TaskStatus::Done)
        }
    }

    #[test]
    fn tree_line_shows_route_triple_not_opaque_cli() {
        let line = format_task_line(&sample(AgentKind::Codex));
        assert!(
            line.contains("codex/openai-chatgpt-plan/unknown"),
            "expected route triple in tree line: {line}"
        );
        assert!(line.contains("1.5k tokens"), "{line}");
    }

    #[test]
    fn tree_line_marks_inferred_attribution() {
        let mut task = sample(AgentKind::Codex);
        task.requested_model = Some("gpt-5.6".to_string());
        task.observed_model = Some("gpt-5.6".to_string());
        task.attribution_source = Some(AttributionSource::ConfirmedBySuccess);
        let line = format_task_line(&task);
        assert!(
            line.contains("gpt-5.6 (inferred)"),
            "attribution grade must ride the model segment: {line}"
        );
    }
}

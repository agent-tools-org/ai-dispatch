// Build a tree of tasks by parent_task_id and workgroup relationships.
// Exports: TreeNode, build_task_tree.
// Deps: crate::types::Task.

use crate::types::Task;

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub task: Task,
    pub depth: usize,
    pub is_last: bool,
    pub prefix: String,
}

pub fn build_task_tree(tasks: &[Task]) -> Vec<TreeNode> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let task_ids: std::collections::HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut roots: Vec<&Task> = tasks
        .iter()
        .filter(|t| match t.parent_task_id.as_deref() {
            None => true,
            Some(pid) => !task_ids.contains(pid),
        })
        .collect();
    roots.sort_by(|a, b| {
        (!b.status.is_terminal())
            .cmp(&!a.status.is_terminal())
            .then(b.created_at.cmp(&a.created_at))
    });
    for root in &roots {
        if seen.contains(root.id.as_str()) {
            continue;
        }
        seen.insert(root.id.as_str().to_string());
        result.push(TreeNode {
            task: (*root).clone(),
            depth: 0,
            is_last: false,
            prefix: String::new(),
        });
        add_children(root.id.as_str(), tasks, &mut result, &mut seen, 1, "");
    }
    fix_last_flags(&mut result);
    result
}

fn add_children(
    parent_id: &str,
    tasks: &[Task],
    result: &mut Vec<TreeNode>,
    seen: &mut std::collections::HashSet<String>,
    depth: usize,
    parent_prefix: &str,
) {
    let children: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.parent_task_id.as_deref() == Some(parent_id) && !seen.contains(t.id.as_str()))
        .collect();
    for (i, child) in children.iter().enumerate() {
        let is_last = i + 1 == children.len();
        seen.insert(child.id.as_str().to_string());
        result.push(TreeNode {
            task: (*child).clone(),
            depth,
            is_last,
            prefix: format!("{parent_prefix}{}", if is_last { "└── " } else { "├── " }),
        });
        add_children(
            child.id.as_str(),
            tasks,
            result,
            seen,
            depth + 1,
            &format!("{parent_prefix}{}", if is_last { "    " } else { "│   " }),
        );
    }
}

fn fix_last_flags(_nodes: &mut [TreeNode]) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;
    fn mk(id: &str, parent: Option<&str>) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            prompt: "test".into(),
            status: TaskStatus::Done,
            parent_task_id: parent.map(str::to_string),
            created_at: Local::now(),
            verify_status: VerifyStatus::Skipped,
            custom_agent_name: None,
            resolved_prompt: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            completed_at: None,
            verify: None,
            read_only: false,
            budget: false,
        }
    }
    #[test]
    fn flat_tasks_no_hierarchy() {
        let tree = build_task_tree(&[mk("t-1", None), mk("t-2", None)]);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].depth, 0);
        assert_eq!(tree[1].depth, 0);
    }
    #[test]
    fn parent_child_creates_hierarchy() {
        let tree = build_task_tree(&[mk("p", None), mk("c1", Some("p")), mk("c2", Some("p"))]);
        assert_eq!(tree.len(), 3);
        assert_eq!(tree[0].depth, 0);
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].depth, 1);
    }
    #[test]
    fn nested_hierarchy() {
        let tree = build_task_tree(&[mk("r", None), mk("m", Some("r")), mk("l", Some("m"))]);
        assert_eq!(tree.len(), 3);
        assert_eq!(tree[0].depth, 0);
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].depth, 2);
    }
}

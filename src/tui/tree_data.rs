// Build a tree of tasks by parent_task_id and workgroup relationships.
// Exports: TreeNode, build_task_tree.
// Deps: crate::types::Task.

use crate::types::{Task, TaskStatus};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub task: Task,
    pub depth: usize,
    pub is_last: bool,
    pub prefix: String,
    /// True if this is a workgroup header (virtual node reusing first task)
    pub is_group_header: bool,
}

/// Build a flat list of TreeNodes with proper indentation.
/// Groups tasks by workgroup, then by parent_task_id hierarchy within each group.
/// Orphan tasks (no workgroup, no parent) appear at root level.
pub fn build_task_tree(tasks: &[Task]) -> Vec<TreeNode> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let task_ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();

    // Group tasks by workgroup_id
    let mut groups: HashMap<Option<&str>, Vec<&Task>> = HashMap::new();
    for task in tasks {
        groups.entry(task.workgroup_id.as_deref()).or_default().push(task);
    }

    // Collect and sort group keys: named groups first (sorted), then None (ungrouped)
    let mut group_keys: Vec<Option<&str>> = groups.keys().copied().collect();
    group_keys.sort_by(|a, b| match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    for group_key in group_keys {
        let group_tasks = &groups[&group_key];

        if let Some(gid) = group_key {
            // Find roots within this group
            let roots = find_roots(group_tasks, &task_ids);
            if roots.is_empty() { continue; }

            // Use first root as group header display
            let header_task = roots[0];
            result.push(TreeNode {
                task: header_task.clone(),
                depth: 0,
                is_last: false,
                prefix: format!("▸ {gid} "),
                is_group_header: true,
            });
            seen.insert(header_task.id.as_str().to_string());

            // Add remaining roots and all children at depth 1+
            for (i, root) in roots.iter().enumerate() {
                if seen.contains(root.id.as_str()) {
                    // header task already added — add its children
                    add_children(root.id.as_str(), group_tasks, &mut result, &mut seen, 1, "  ");
                    continue;
                }
                let is_last = i + 1 == roots.len();
                let connector = if is_last { "  └── " } else { "  ├── " };
                seen.insert(root.id.as_str().to_string());
                result.push(TreeNode {
                    task: (*root).clone(),
                    depth: 1,
                    is_last,
                    prefix: connector.to_string(),
                    is_group_header: false,
                });
                let next_prefix = if is_last { "      " } else { "  │   " };
                add_children(root.id.as_str(), group_tasks, &mut result, &mut seen, 2, next_prefix);
            }
        } else {
            // Ungrouped tasks — flat roots with parent-child hierarchy
            let all_refs: Vec<&Task> = tasks.iter().collect();
            let mut roots = find_roots(group_tasks, &task_ids);
            roots.sort_by(|a, b| {
                b.status.is_terminal().cmp(&a.status.is_terminal())
                    .then(b.created_at.cmp(&a.created_at))
            });
            for root in &roots {
                if seen.contains(root.id.as_str()) { continue; }
                seen.insert(root.id.as_str().to_string());
                result.push(TreeNode {
                    task: (*root).clone(),
                    depth: 0,
                    is_last: false,
                    prefix: String::new(),
                    is_group_header: false,
                });
                add_children(root.id.as_str(), &all_refs, &mut result, &mut seen, 1, "");
            }
        }
    }
    result
}

fn find_roots<'a>(tasks: &[&'a Task], all_ids: &HashSet<&str>) -> Vec<&'a Task> {
    let mut roots: Vec<&Task> = tasks
        .iter()
        .filter(|t| match t.parent_task_id.as_deref() {
            None => true,
            Some(pid) => !all_ids.contains(pid),
        })
        .copied()
        .collect();
    roots.sort_by(|a, b| {
        let a_active = !a.status.is_terminal();
        let b_active = !b.status.is_terminal();
        b_active.cmp(&a_active).then(b.created_at.cmp(&a.created_at))
    });
    roots
}

fn add_children(
    parent_id: &str,
    tasks: &[&Task],
    result: &mut Vec<TreeNode>,
    seen: &mut HashSet<String>,
    depth: usize,
    parent_prefix: &str,
) {
    let children: Vec<&&Task> = tasks
        .iter()
        .filter(|t| t.parent_task_id.as_deref() == Some(parent_id) && !seen.contains(t.id.as_str()))
        .collect();
    for (i, child) in children.iter().enumerate() {
        let is_last = i + 1 == children.len();
        seen.insert(child.id.as_str().to_string());
        result.push(TreeNode {
            task: (**child).clone(),
            depth,
            is_last,
            prefix: format!("{parent_prefix}{}", if is_last { "└── " } else { "├── " }),
            is_group_header: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId, VerifyStatus};
    use chrono::Local;

    fn mk(id: &str, parent: Option<&str>) -> Task {
        mk_group(id, parent, None)
    }

    fn mk_group(id: &str, parent: Option<&str>, group: Option<&str>) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            prompt: "test".into(),
            status: TaskStatus::Done,
            parent_task_id: parent.map(str::to_string),
            workgroup_id: group.map(str::to_string),
            created_at: Local::now(),
            verify_status: VerifyStatus::Skipped,
            custom_agent_name: None, resolved_prompt: None,
            caller_kind: None, caller_session_id: None, agent_session_id: None,
            repo_path: None, worktree_path: None, worktree_branch: None,
            log_path: None, output_path: None, tokens: None, prompt_tokens: None,
            duration_ms: None, model: None, cost_usd: None, exit_code: None,
            completed_at: None, verify: None, read_only: false, budget: false,
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

    #[test]
    fn workgroup_tasks_grouped() {
        let tasks = vec![
            mk_group("t-1", None, Some("wg-a")),
            mk_group("t-2", None, Some("wg-a")),
            mk("t-3", None),
        ];
        let tree = build_task_tree(&tasks);
        // wg-a group header + t-2 child + ungrouped t-3
        assert!(tree[0].is_group_header);
        assert!(tree[0].prefix.contains("wg-a"));
        assert!(!tree.last().unwrap().is_group_header);
    }
}

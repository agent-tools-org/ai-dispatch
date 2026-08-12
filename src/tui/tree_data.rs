// Build a tree of tasks by project, then parent_task_id hierarchy.
// Exports: TreeNode, build_task_tree.
// Deps: crate::types::Task.

use crate::types::Task;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub task: Task,
    #[allow(dead_code)]
    pub depth: usize,
    pub prefix: String,
    /// True if this is a workgroup header (virtual node reusing first task)
    pub is_group_header: bool,
}

/// Build a flat list of TreeNodes with proper indentation.
/// When multiple projects are present, groups by project_id (NULL → unattributed).
/// Within a project, nests by parent_task_id.
#[cfg(test)]
pub fn build_task_tree(tasks: &[Task]) -> Vec<TreeNode> {
    build_task_tree_with_creators(tasks, &HashMap::new())
}

/// Build tree with optional workgroup creator labels.
pub fn build_task_tree_with_creators(tasks: &[Task], creators: &HashMap<String, String>) -> Vec<TreeNode> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let task_ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();

    // Group tasks by project_id (first-class). NULL → unattributed bucket.
    let mut groups: HashMap<Option<&str>, Vec<&Task>> = HashMap::new();
    for task in tasks {
        groups.entry(task.project_id.as_deref()).or_default().push(task);
    }

    // Named projects first (newest first), unattributed last.
    let mut group_keys: Vec<Option<&str>> = groups.keys().copied().collect();
    group_keys.sort_by(|a, b| match (a, b) {
        (Some(ga), Some(gb)) => {
            let newest = |g: &str| groups[&Some(g)].iter().map(|t| t.created_at).max();
            newest(gb).cmp(&newest(ga))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let multi_project = groups.len() > 1;
    for group_key in group_keys {
        let group_tasks = &groups[&group_key];
        let project_label = crate::project::project_display(group_key);

        if multi_project {
            // Find roots within this project
            let roots = find_roots(group_tasks, &task_ids);
            if roots.is_empty() { continue; }

            // Use first root as project header display
            let header_task = roots[0];
            let wg_hint = header_task
                .workgroup_id
                .as_deref()
                .and_then(|gid| creators.get(gid).map(|by| format!(" ({gid}/{by})")))
                .or_else(|| {
                    header_task
                        .workgroup_id
                        .as_ref()
                        .map(|gid| format!(" ({gid})"))
                })
                .unwrap_or_default();
            result.push(TreeNode {
                task: header_task.clone(),
                depth: 0,
                prefix: format!("▸ {project_label}{wg_hint} "),
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
                    prefix: connector.to_string(),
                    is_group_header: false,
                });
                let next_prefix = if is_last { "      " } else { "  │   " };
                add_children(root.id.as_str(), group_tasks, &mut result, &mut seen, 2, next_prefix);
            }
        } else {
            // Single project view — flat roots with parent-child hierarchy
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
    use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};
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
            repo_path: None, project_id: crate::project::current_project_id(), worktree_path: None, worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
            start_sha: None,
            log_path: None, output_path: None, tokens: None, prompt_tokens: None,
            duration_ms: None, requested_model: None, observed_model: None, attribution_source: None, cost_usd: None, exit_code: None,
            completed_at: None, verify: None, pending_reason: None, read_only: false, budget: false,
            audit_verdict: None, audit_report_path: None, delivery_assessment: None,
            category: None,
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
    fn multi_project_tasks_grouped() {
        let mut a = mk("t-1", None);
        a.project_id = Some("proj-a".into());
        let mut b = mk("t-2", None);
        b.project_id = Some("proj-b".into());
        let mut u = mk("t-3", None);
        u.project_id = None;
        let tree = build_task_tree(&[a, b, u]);
        let headers: Vec<_> = tree
            .iter()
            .filter(|n| n.is_group_header)
            .map(|n| n.prefix.clone())
            .collect();
        assert!(headers.iter().any(|p| p.contains("proj-a")), "{headers:?}");
        assert!(headers.iter().any(|p| p.contains("proj-b")), "{headers:?}");
        assert!(
            headers.iter().any(|p| p.contains("unattributed")),
            "{headers:?}"
        );
    }
}

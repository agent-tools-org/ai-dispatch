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
    /// True if this is a project header (virtual node reusing the first task).
    pub is_group_header: bool,
    pub project_id: Option<String>,
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
    build_task_tree_with_state(tasks, creators, &HashSet::new())
}

/// Build the grouped task rows used by the board and tree views.
pub fn build_task_tree_with_state(
    tasks: &[Task],
    creators: &HashMap<String, String>,
    collapsed_projects: &HashSet<Option<String>>,
) -> Vec<TreeNode> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();

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

    for group_key in group_keys {
        let group_tasks = &groups[&group_key];
        let project_label = crate::project::project_display(group_key);
        let group_ids: HashSet<&str> = group_tasks.iter().map(|task| task.id.as_str()).collect();
        let roots = find_roots(group_tasks, &group_ids);
        let Some(header_task) = roots.first().or_else(|| group_tasks.first()) else {
            continue;
        };
        let group_id = group_key.map(str::to_string);
        let collapsed = collapsed_projects.contains(&group_id);
        let marker = if collapsed { "▸" } else { "▾" };
        let total = group_tasks.len();
        let done = group_tasks.iter().filter(|task| task.status.is_terminal()).count();
        let running = group_tasks.iter().filter(|task| task.status == crate::types::TaskStatus::Running).count();
        let workgroup_hint = header_task
            .workgroup_id
            .as_deref()
            .and_then(|group| creators.get(group).map(|creator| format!(" ({group}/{creator})")))
            .or_else(|| header_task.workgroup_id.as_ref().map(|group| format!(" ({group})")))
            .unwrap_or_default();
        let running_hint = if running > 0 { format!(" {running}▶") } else { String::new() };
        result.push(TreeNode {
            task: (*header_task).clone(),
            depth: 0,
            prefix: format!("{marker} {project_label}{workgroup_hint}{running_hint} ({done}/{total}) "),
            is_group_header: true,
            project_id: group_id.clone(),
        });
        if collapsed {
            continue;
        }
        for (i, root) in roots.iter().enumerate() {
            if seen.contains(root.id.as_str()) {
                continue;
            }
            seen.insert(root.id.as_str().to_string());
            let is_last = i + 1 == roots.len();
            let connector = if is_last { "  └── " } else { "  ├── " };
            result.push(TreeNode {
                task: (*root).clone(),
                depth: 1,
                prefix: connector.to_string(),
                is_group_header: false,
                project_id: group_id.clone(),
            });
            let next_prefix = if is_last { "      " } else { "  │   " };
            add_children(root.id.as_str(), group_tasks, &mut result, &mut seen, 2, next_prefix, &group_id);
        }
        // A malformed retry cycle must not make a task disappear from the TUI.
        let remaining: Vec<&Task> = group_tasks
            .iter()
            .filter(|task| !seen.contains(task.id.as_str()))
            .copied()
            .collect();
        for task in remaining {
            seen.insert(task.id.as_str().to_string());
            result.push(TreeNode {
                task: (*task).clone(),
                depth: 1,
                prefix: "  └── ".to_string(),
                is_group_header: false,
                project_id: group_id.clone(),
            });
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
    project_id: &Option<String>,
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
            project_id: project_id.clone(),
        });
        add_children(
            child.id.as_str(),
            tasks,
            result,
            seen,
            depth + 1,
            &format!("{parent_prefix}{}", if is_last { "    " } else { "│   " }),
            project_id,
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
        assert_eq!(tree.len(), 3);
        assert!(tree[0].is_group_header);
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].depth, 1);
    }

    #[test]
    fn parent_child_creates_hierarchy() {
        let tree = build_task_tree(&[mk("p", None), mk("c1", Some("p")), mk("c2", Some("p"))]);
        assert_eq!(tree.len(), 4);
        assert!(tree[0].is_group_header);
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].depth, 2);
        assert_eq!(tree[3].depth, 2);
    }

    #[test]
    fn nested_hierarchy() {
        let tree = build_task_tree(&[mk("r", None), mk("m", Some("r")), mk("l", Some("m"))]);
        assert_eq!(tree.len(), 4);
        assert!(tree[0].is_group_header);
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].depth, 2);
        assert_eq!(tree[3].depth, 3);
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

    #[test]
    fn grouped_rows_show_every_task_and_keep_unattributed_separate() {
        let mut a = mk("a", None);
        a.project_id = Some("alpha".into());
        let mut b = mk("b", None);
        b.project_id = Some("beta".into());
        let mut u = mk("u", None);
        u.project_id = None;
        let tree = build_task_tree(&[a, b, u]);

        let task_ids: HashSet<&str> = tree
            .iter()
            .filter(|node| !node.is_group_header)
            .map(|node| node.task.id.as_str())
            .collect();
        assert_eq!(task_ids, HashSet::from(["a", "b", "u"]));
        assert_eq!(tree.iter().filter(|node| node.is_group_header).count(), 3);
        assert!(tree.iter().any(|node| {
            node.is_group_header && node.project_id.is_none() && node.prefix.contains("unattributed")
        }));
    }

    #[test]
    fn collapsed_group_header_keeps_done_total_count_visible() {
        let mut task = mk("alpha-task", None);
        task.project_id = Some("alpha".into());
        let collapsed = HashSet::from([Some("alpha".to_string())]);
        let tree = build_task_tree_with_state(&[task], &HashMap::new(), &collapsed);

        assert_eq!(tree.len(), 1);
        assert!(tree[0].prefix.contains("(1/1)"), "{}", tree[0].prefix);
    }
}

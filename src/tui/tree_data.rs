// Build a tree of tasks by project, then parent_task_id hierarchy.
// Exports: TreeNode, build_task_tree.
// Deps: crate::types::Task.

use crate::types::Task;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub task_index: usize,
    pub task_id: crate::types::TaskId,
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
#[cfg(test)]
pub fn build_task_tree_with_creators(tasks: &[Task], creators: &HashMap<String, String>) -> Vec<TreeNode> {
    build_task_tree_with_state(tasks, creators, &HashSet::new())
}

/// Build the grouped task rows used by the board and tree views.
pub fn build_task_tree_with_state(
    tasks: &[Task],
    creators: &HashMap<String, String>,
    collapsed_projects: &HashSet<Option<String>>,
) -> Vec<TreeNode> {
    let indices: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
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
        append_group(&groups[&group_key], group_key, creators, collapsed_projects,
            &indices, &mut result, &mut seen);
    }
    result
}

fn append_group(
    tasks: &[&Task], project: Option<&str>, creators: &HashMap<String, String>,
    collapsed_projects: &HashSet<Option<String>>, indices: &HashMap<&str, usize>,
    result: &mut Vec<TreeNode>, seen: &mut HashSet<String>,
) {
    let ids: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    let roots = find_roots(tasks, &ids);
    let Some(header) = roots.first().or_else(|| tasks.first()) else { return; };
    let project_id = project.map(str::to_string);
    let collapsed = collapsed_projects.contains(&project_id);
    result.push(TreeNode {
        task_index: indices[header.id.as_str()], task_id: header.id.clone(), depth: 0,
        prefix: group_label(tasks, header, project, creators, collapsed),
        is_group_header: true, project_id: project_id.clone(),
    });
    if collapsed { return; }
    let mut children: HashMap<&str, Vec<&Task>> = HashMap::new();
    for task in tasks {
        if let Some(parent) = task.parent_task_id.as_deref() {
            children.entry(parent).or_default().push(task);
        }
    }
    for (i, root) in roots.iter().enumerate() {
        if !seen.insert(root.id.to_string()) { continue; }
        let last = i + 1 == roots.len();
        result.push(TreeNode {
            task_index: indices[root.id.as_str()], task_id: root.id.clone(), depth: 1,
            prefix: if last { "  └── " } else { "  ├── " }.into(),
            is_group_header: false, project_id: project_id.clone(),
        });
        let prefix = if last { "      " } else { "  │   " };
        add_children(root.id.as_str(), &children, indices, result, seen, 2, prefix, &project_id);
    }
    // A malformed retry cycle must not make a task disappear from the TUI.
    for task in tasks {
        if !seen.insert(task.id.to_string()) { continue; }
        result.push(TreeNode {
            task_index: indices[task.id.as_str()], task_id: task.id.clone(), depth: 1,
            prefix: "  └── ".into(), is_group_header: false, project_id: project_id.clone(),
        });
    }
}

fn group_label(
    tasks: &[&Task], header: &Task, project: Option<&str>,
    creators: &HashMap<String, String>, collapsed: bool,
) -> String {
    let label = crate::project::project_display(project);
    let marker = if collapsed { "▸" } else { "▾" };
    let total = tasks.len();
    let done = tasks.iter().filter(|task| task.status.is_terminal()).count();
    let running = tasks.iter().filter(|task| task.status == crate::types::TaskStatus::Running).count();
    let workgroup = header.workgroup_id.as_deref().map(|group| {
        creators.get(group).map(|creator| format!(" ({group}/{creator})"))
            .unwrap_or_else(|| format!(" ({group})"))
    }).unwrap_or_default();
    let running_hint = if running > 0 { format!(" {running}▶") } else { String::new() };
    format!("{marker} {label}{workgroup}{running_hint} ({done}/{total}) ")
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
    children_by_parent: &HashMap<&str, Vec<&Task>>,
    indices: &HashMap<&str, usize>,
    result: &mut Vec<TreeNode>,
    seen: &mut HashSet<String>,
    depth: usize,
    parent_prefix: &str,
    project_id: &Option<String>,
) {
    let Some(children) = children_by_parent.get(parent_id) else { return; };
    for (i, child) in children.iter().enumerate() {
        if seen.contains(child.id.as_str()) { continue; }
        let is_last = i + 1 == children.len();
        seen.insert(child.id.as_str().to_string());
        result.push(TreeNode {
            task_index: indices[child.id.as_str()],
            task_id: child.id.clone(),
            depth,
            prefix: format!("{parent_prefix}{}", if is_last { "└── " } else { "├── " }),
            is_group_header: false,
            project_id: project_id.clone(),
        });
        add_children(
            child.id.as_str(),
            children_by_parent,
            indices,
            result,
            seen,
            depth + 1,
            &format!("{parent_prefix}{}", if is_last { "    " } else { "│   " }),
            project_id,
        );
    }
}

#[cfg(test)]
#[path = "tree_data_tests.rs"]
mod tests;

// Tree grouping and hierarchy regression tests.
// Depends on tree_data and task fixtures.

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
            repo_path: None, project_id: crate::project::current_project_id(), worktree_path: None, effective_dir: None, worktree_branch: None,
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
            .map(|node| node.task_id.as_str())
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

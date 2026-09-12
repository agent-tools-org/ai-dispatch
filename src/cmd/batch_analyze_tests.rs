// Batch analysis regression tests and task fixtures.
// Deps: parent analysis helpers and BatchTask.
    use super::*;

    fn stub_task(name: &str, prompt: &str) -> BatchTask {
        BatchTask {
            id: None,
            name: Some(name.to_string()),
            agent: "codex".to_string(),
            team: None,
            prompt: prompt.to_string(),
            prompt_file: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            group: None,
            container: None, remote_build: None,
            verify: None,
            setup: None,
            judge: None,
            peer_review: None,
            best_of: None,
            max_duration_mins: None,
            max_wait_mins: None,
            retry: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            idle_timeout: None,
            metric: None,
            context: None,
            checklist: None,
            skills: None,
            on_done: None,
            hooks: None,
            depends_on: None,
            parent: None,
            context_from: None,
            fallback: None,
            scope: None,
            read_only: false,
            sandbox: false,
            no_skill: false,
            difficulty: None, budget: None, urgency: None, rigor: None, egress: None, kind: None,
            audit: None,
            env: None,
            env_forward: None,
            worktree_link_deps: None,
            on_success: None,
            on_fail: None,
            conditional: false,
        }
    }

    #[test]
    fn analyze_detects_overlapping_context_files() {
        let mut left = stub_task("task-a", "left");
        left.context = Some(vec!["src/types.rs".to_string()]);
        let mut right = stub_task("task-b", "right");
        right.context = Some(vec!["src/types.rs".to_string()]);
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps, vec![FileOverlap {
            file: "src/types.rs".to_string(),
            task_ids: vec!["task-a".to_string(), "task-b".to_string()],
            severity: OverlapSeverity::Warning,
        }]);
    }

    #[test]
    fn analyze_ignores_tasks_with_dependency() {
        let mut parent = stub_task("task-a", "touch src/types.rs");
        parent.context = Some(vec!["src/types.rs".to_string()]);
        let mut child = stub_task("task-b", "touch src/types.rs");
        child.depends_on = Some(vec!["task-a".to_string()]);
        let overlaps = analyze_file_overlap(&[parent, child], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

    #[test]
    fn analyze_extracts_paths_from_prompt() {
        let left = stub_task("task-a", "Update src/types.rs and keep tests green.");
        let right = stub_task("task-b", "Review src/types.rs for shared changes.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "src/types.rs");
        assert_eq!(overlaps[0].task_ids, vec!["task-a".to_string(), "task-b".to_string()]);
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn analyze_skips_context_for_read_only_tasks() {
        let mut writer = stub_task("writer", "edit something");
        writer.context = Some(vec!["src/main.rs".to_string()]);
        let mut reader = stub_task("reader", "review something");
        reader.context = Some(vec!["src/main.rs".to_string()]);
        reader.read_only = true;
        let overlaps = analyze_file_overlap(&[writer, reader], &BatchDefaults::default());
        assert!(overlaps.is_empty(), "read_only task should not trigger overlap warning");
    }

    #[test]
    fn analyze_no_false_positives_on_urls() {
        let left = stub_task("task-a", "See https://example.com/src/types.rs for context.");
        let right = stub_task("task-b", "No files mentioned here.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

    #[test]
    fn analyze_detects_overlapping_output_files() {
        let mut left = stub_task("task-a", "write findings");
        left.output = Some("FINDINGS.md".to_string());
        let mut right = stub_task("task-b", "write findings");
        right.output = Some("FINDINGS.md".to_string());
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "FINDINGS.md");
        assert_eq!(overlaps[0].task_ids, vec!["task-a".to_string(), "task-b".to_string()]);
        assert_eq!(overlaps[0].severity, OverlapSeverity::Error);
    }

    #[test]
    fn analyze_output_overlap_ignored_with_dependency() {
        let mut parent = stub_task("task-a", "write findings");
        parent.output = Some("FINDINGS.md".to_string());
        let mut child = stub_task("task-b", "write findings");
        child.output = Some("FINDINGS.md".to_string());
        child.depends_on = Some(vec!["task-a".to_string()]);
        let overlaps = analyze_file_overlap(&[parent, child], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

    #[test]
    fn analyze_detects_md_paths_in_prompt() {
        let left = stub_task("task-a", "Write findings to FINDINGS.md.");
        let right = stub_task("task-b", "Review FINDINGS.md before merge.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "FINDINGS.md");
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn analyze_detects_json_paths_in_prompt() {
        let left = stub_task("task-a", "Write results.json for the run.");
        let right = stub_task("task-b", "Validate results.json structure.");
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "results.json");
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn analyze_detects_result_file_overlap() {
        let mut left = stub_task("task-a", "audit left");
        left.result_file = Some("result.md".to_string());
        let mut right = stub_task("task-b", "audit right");
        right.result_file = Some("result.md".to_string());
        let overlaps = analyze_file_overlap(&[left, right], &BatchDefaults::default());
        assert_eq!(overlaps[0].file, "result.md");
        assert_eq!(overlaps[0].severity, OverlapSeverity::Error);
    }

    #[test]
    fn analyze_result_file_overlap_ignored_with_dependency() {
        let mut parent = stub_task("task-a", "audit left");
        parent.result_file = Some("result.md".to_string());
        let mut child = stub_task("task-b", "audit right");
        child.result_file = Some("result.md".to_string());
        child.depends_on = Some(vec!["task-a".to_string()]);
        let overlaps = analyze_file_overlap(&[parent, child], &BatchDefaults::default());
        assert!(overlaps.is_empty());
    }

// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn rejects_unknown_agent() {
    let file = write_temp("[[tasks]]\nagent = \"gpt-3\"\nprompt = \"do something\"");
    assert!(parse_batch_file(file.path())
        .unwrap_err()
        .to_string()
        .contains("unknown agent"));
}

#[test]
fn auto_sequences_shared_worktree_tasks() {
    let file = write_temp(concat!(
        "[[tasks]]\nname = \"task-a\"\nagent = \"gemini\"\nprompt = \"a\"\nworktree = \"feat/x\"\n",
        "[[tasks]]\nname = \"task-b\"\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/x\""
    ));
    let cfg = parse_batch_file(file.path()).unwrap();
    assert_eq!(
        cfg.tasks[1].depends_on.as_deref(),
        Some(&["task-a".to_string()][..]),
        "task-b should auto-depend on task-a"
    );
}

#[test]
fn auto_sequence_preserves_existing_depends_on() {
    let file = write_temp(concat!(
        "[[tasks]]\nname = \"task-a\"\nagent = \"codex\"\nprompt = \"a\"\nworktree = \"feat/x\"\n",
        "[[tasks]]\nname = \"task-b\"\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/x\"\n",
        "depends_on = [\"task-a\"]"
    ));
    let cfg = parse_batch_file(file.path()).unwrap();
    assert_eq!(cfg.tasks[1].depends_on.as_ref().unwrap().len(), 1);
}

#[test]
fn auto_sequence_three_tasks_creates_chain() {
    let file = write_temp(concat!(
        "[[tasks]]\nname = \"a\"\nagent = \"codex\"\nprompt = \"1\"\nworktree = \"feat/x\"\n",
        "[[tasks]]\nname = \"b\"\nagent = \"codex\"\nprompt = \"2\"\nworktree = \"feat/x\"\n",
        "[[tasks]]\nname = \"c\"\nagent = \"codex\"\nprompt = \"3\"\nworktree = \"feat/x\""
    ));
    let cfg = parse_batch_file(file.path()).unwrap();
    assert!(cfg.tasks[0].depends_on.is_none(), "first task has no deps");
    assert_eq!(cfg.tasks[1].depends_on.as_deref(), Some(&["a".to_string()][..]));
    assert_eq!(cfg.tasks[2].depends_on.as_deref(), Some(&["b".to_string()][..]));
}

#[test]
fn warns_on_large_prompt() {
    let big_prompt = "x".repeat(7000);
    let task = BatchTask {
        prompt: big_prompt,
        prompt_file: None,
        ..make_task(Some("huge"), &[])
    };
    let mut output = Vec::new();
    warn_prompt_size(&[task], &mut output).unwrap();
    let msg = String::from_utf8(output).unwrap();
    assert!(msg.contains("large prompt"), "should warn about large prompt");
    assert!(msg.contains("huge"), "should name the task");
}

#[test]
fn no_warning_on_normal_prompt_size() {
    let task = make_task(Some("small"), &[]);
    let mut output = Vec::new();
    warn_prompt_size(&[task], &mut output).unwrap();
    assert!(output.is_empty());
}

#[test]
fn rejects_empty_batch() {
    let err = parse_batch_file(write_temp("").path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("parse TOML") || err.contains("no tasks"));
}

#[test]
fn rejects_invalid_dependency_reference() {
    let err = validate_dag(&[make_task(Some("feature"), &["missing"])])
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown task"));
}

#[test]
fn rejects_dependency_cycles() {
    let tasks = vec![
        make_task(Some("foundation"), &["integration"]),
        make_task(Some("integration"), &["foundation"]),
    ];
    let err = validate_dag(&tasks).unwrap_err().to_string();
    assert!(err.contains("cycle"));
}

#[test]
fn rejects_unknown_fallback_agent() {
    let file = write_temp(concat!(
        "[[tasks]]\nagent = \"codex\"\nprompt = \"do something\"\n",
        "fallback = \"codex,unknown-agent\""
    ));
    assert!(parse_batch_file(file.path())
        .unwrap_err()
        .to_string()
        .contains("unknown fallback agent"));
}

#[test]
fn accepts_valid_fallback_agent() {
    let file = write_temp(concat!(
        "[[tasks]]\nagent = \"codex\"\nprompt = \"do something\"\n",
        "fallback = \"opencode\""
    ));
    assert!(parse_batch_file(file.path()).is_ok());
}

#[test]
fn accepts_comma_separated_fallback() {
    let toml = r#"
[[tasks]]
prompt = "test"
fallback = "codex,opencode"
"#;
    let config: BatchConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.tasks[0].fallback.as_deref(), Some("codex,opencode"));
}

#[test]
fn accepts_tasks_plural_alias() {
    let file = write_temp(concat!(
        "[[tasks]]\nagent = \"gemini\"\nprompt = \"research\"\n",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"implement\""
    ));
    let cfg = parse_batch_file(file.path()).unwrap();
    assert_eq!(cfg.tasks.len(), 2);
}

#[test]
fn accepts_task_singular_alias() {
    let file = write_temp("[[task]]\nagent = \"codex\"\nprompt = \"implement\"\n");
    let cfg = parse_batch_file(file.path()).unwrap();
    assert_eq!(cfg.tasks.len(), 1);
}

#[test]
fn rejects_unknown_top_level_key() {
    let file = write_temp("[bogus]\nfoo = 1\n\n[[tasks]]\nagent = \"codex\"\nprompt = \"implement\"\n");
    let err = parse_batch_file(file.path()).unwrap_err().to_string();
    assert!(err.contains("unknown top-level key `bogus`"));
}

#[test]
fn rejects_unknown_metadata_top_level_key() {
    let file = write_temp("titl = \"typo\"\n\n[[tasks]]\nagent = \"codex\"\nprompt = \"implement\"\n");
    let err = parse_batch_file(file.path()).unwrap_err().to_string();

    assert!(err.contains("unknown top-level key `titl`"));
}

#[test]
fn rejects_unknown_section() {
    let file = write_temp(concat!(
        "[setting]\nagent = \"codex\"\n",
        "[[tasks]]\nprompt = \"implement\"\n"
    ));
    let err = parse_batch_file(file.path()).unwrap_err().to_string();

    assert!(err.contains("unknown top-level key `setting`"));
}

#[test]
fn accepts_valid_sections() {
    let file = write_temp(concat!(
        "[defaults]\nagent = \"codex\"\n",
        "[vars]\nproject = \"demo\"\n",
        "[[tasks]]\nprompt = \"build {{project}}\"\n"
    ));

    assert!(parse_batch_file(file.path()).is_ok());
}

#[test]
fn context_from_creates_implicit_dependency() {
    let a = make_task(Some("research"), &[]);
    let mut b = make_task(Some("implement"), &[]);
    b.context_from = Some(vec!["research".to_string()]);
    let tasks = vec![a, b];
    let deps = dependency_indices(&tasks).unwrap();
    assert!(deps[0].is_empty());
    assert_eq!(
        deps[1],
        vec![0],
        "context_from should create implicit dependency"
    );
}

#[test]
fn context_from_deduplicates_with_explicit_depends_on() {
    let a = make_task(Some("research"), &[]);
    let mut b = make_task(Some("implement"), &["research"]);
    b.context_from = Some(vec!["research".to_string()]);
    let tasks = vec![a, b];
    let deps = dependency_indices(&tasks).unwrap();
    assert_eq!(
        deps[1],
        vec![0],
        "duplicate dependency should be deduplicated"
    );
}

#[test]
fn warns_on_audit_prompt_without_read_only() {
    let task = BatchTask {
        prompt: "Audit this codebase and report only findings".to_string(),
        prompt_file: None,
        ..make_task(Some("review"), &[])
    };
    let mut stderr = Vec::new();

    warn_audit_without_readonly_into(&[task], &mut stderr).unwrap();

    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("Task 'review' prompt suggests read-only intent"));
}

#[test]
fn does_not_warn_on_normal_prompt() {
    let task = BatchTask {
        prompt: "Implement the parser changes".to_string(),
        prompt_file: None,
        ..make_task(Some("implement"), &[])
    };
    let mut stderr = Vec::new();

    warn_audit_without_readonly_into(&[task], &mut stderr).unwrap();

    assert!(stderr.is_empty());
}

#[test]
fn does_not_warn_when_read_only_is_true() {
    let task = BatchTask {
        prompt: "Do not modify files, analysis only".to_string(),
        prompt_file: None,
        read_only: true,
        ..make_task(Some("analysis"), &[])
    };
    let mut stderr = Vec::new();

    warn_audit_without_readonly_into(&[task], &mut stderr).unwrap();

    assert!(stderr.is_empty());
}

#[test]
fn does_not_warn_for_audit_log_prompt() {
    let task = BatchTask {
        prompt: "Add an audit log feature for admin actions".to_string(),
        prompt_file: None,
        ..make_task(Some("feature"), &[])
    };
    let mut stderr = Vec::new();

    warn_audit_without_readonly_into(&[task], &mut stderr).unwrap();

    assert!(stderr.is_empty());
}

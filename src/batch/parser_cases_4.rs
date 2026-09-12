// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn judge_true_defaults_to_gemini() {
    let cfg = parse_batch_file(
        write_temp("[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\njudge = true\n").path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks[0].judge.as_deref(), Some("gemini"));
}

#[test]
fn judge_string_uses_specified_agent() {
    let cfg = parse_batch_file(
        write_temp("[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\njudge = \"cursor\"\n").path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks[0].judge.as_deref(), Some("cursor"));
}

#[test]
fn judge_false_is_none() {
    let cfg = parse_batch_file(
        write_temp("[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\njudge = false\n").path(),
    )
    .unwrap();

    assert!(cfg.tasks[0].judge.is_none());
}

#[test]
fn judge_absent_is_none() {
    let cfg =
        parse_batch_file(write_temp("[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\n").path())
            .unwrap();

    assert!(cfg.tasks[0].judge.is_none());
}

#[test]
fn judge_defaults_propagate_to_tasks() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\njudge = true\nagent = \"codex\"\n",
            "[[tasks]]\nprompt = \"test\"\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.defaults.judge.as_deref(), Some("gemini"));
    assert_eq!(cfg.tasks[0].judge.as_deref(), Some("gemini"));
}

#[test]
fn interpolates_task_vars_in_prompt_dir_and_worktree() {
    let (cfg, stderr) = parse_batch_with_vars(
        concat!(
            "[vars]\nproject_name = \"my-app\"\nbase_dir = \"/tmp/projects\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"Build {{project_name}}\"\n",
            "dir = \"{{base_dir}}/{{project_name}}\"\n",
            "worktree = \"feat/{{project_name}}\"\n"
        ),
        &[],
    );

    let task = &cfg.tasks[0];
    assert_eq!(task.prompt, "Build my-app");
    assert_eq!(task.dir.as_deref(), Some("/tmp/projects/my-app"));
    assert_eq!(task.worktree.as_deref(), Some("feat/my-app"));
    assert!(stderr.is_empty());
}

#[test]
fn cli_vars_override_toml_vars() {
    let (cfg, stderr) = parse_batch_with_vars(
        concat!(
            "[vars]\nproject_name = \"from-toml\"\nbase_dir = \"/tmp/projects\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"Build {{project_name}}\"\n"
        ),
        &[("project_name", "from-cli")],
    );

    assert_eq!(cfg.tasks[0].prompt, "Build from-cli");
    assert!(stderr.is_empty());
}

#[test]
fn missing_var_warns_without_failing() {
    let (cfg, stderr) = parse_batch_with_vars(
        "[[task]]\nagent = \"codex\"\nprompt = \"Build {{missing}}\"\n",
        &[],
    );

    assert_eq!(cfg.tasks[0].prompt, "Build {{missing}}");
    assert!(stderr.contains("missing batch var 'missing'"));
}

#[test]
fn no_vars_section_keeps_existing_behavior() {
    let (cfg, stderr) = parse_batch_with_vars(
        "[[task]]\nagent = \"codex\"\nprompt = \"do something\"\n",
        &[],
    );

    assert_eq!(cfg.tasks[0].prompt, "do something");
    assert!(stderr.is_empty());
}

#[test]
fn resolves_prompt_file_relative_to_batch_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let prompt_dir = dir.path().join("prompts");
    std::fs::create_dir_all(&prompt_dir).unwrap();
    std::fs::write(prompt_dir.join("fix.md"), "Prompt from relative file").unwrap();
    let batch_path = dir.path().join("tasks.toml");
    std::fs::write(
        &batch_path,
        "[[tasks]]\nagent = \"codex\"\nprompt_file = \"prompts/fix.md\"\n",
    )
    .unwrap();

    let cfg = parse_batch_file(&batch_path).unwrap();

    assert_eq!(cfg.tasks[0].prompt, "Prompt from relative file");
    assert_eq!(cfg.tasks[0].prompt_file.as_deref(), Some("prompts/fix.md"));
}

#[test]
fn rejects_task_without_prompt_or_prompt_file() {
    let err = parse_batch_file(write_temp("[[tasks]]\nagent = \"codex\"\n").path())
        .unwrap_err()
        .to_string();

    assert!(err.contains("must set either prompt or prompt_file"));
}

#[test]
fn rejects_task_with_prompt_and_prompt_file() {
    let err = parse_batch_file(
        write_temp(
            "[[tasks]]\nagent = \"codex\"\nprompt = \"inline\"\nprompt_file = \"prompts/fix.md\"\n",
        )
        .path(),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("cannot set both prompt and prompt_file"));
}

#[test]
fn warns_on_dir_overlap_without_worktree() {
    let mut task1 = make_task(Some("task1"), &[]);
    task1.dir = Some("src".to_string());
    let mut task2 = make_task(Some("task2"), &[]);
    task2.dir = Some("src".to_string());

    let warnings = warn_dir_overlap(&[task1, task2]);

    assert!(!warnings.is_empty());
    assert!(warnings[0].contains("2 tasks target dir 'src' without worktree isolation"));
    assert!(warnings.iter().any(|w| w.contains("worktree")));
}

#[test]
fn no_warning_when_worktree_set() {
    let mut task1 = make_task(Some("task1"), &[]);
    task1.dir = Some("src".to_string());
    task1.worktree = Some("branch1".to_string());
    let mut task2 = make_task(Some("task2"), &[]);
    task2.dir = Some("src".to_string());
    task2.worktree = Some("branch2".to_string());

    let warnings = warn_dir_overlap(&[task1, task2]);

    assert!(warnings.is_empty());
}

#[test]
fn single_task_no_warning() {
    let mut task = make_task(Some("task1"), &[]);
    task.dir = Some("src".to_string());

    let warnings = warn_dir_overlap(&[task]);

    assert!(warnings.is_empty());
}

#[test]
fn mixed_worktree_no_warning_for_isolated() {
    let mut task1 = make_task(Some("task1"), &[]);
    task1.dir = Some("src".to_string());
    let mut task2 = make_task(Some("task2"), &[]);
    task2.dir = Some("src".to_string());
    task2.worktree = Some("branch2".to_string());

    let warnings = warn_dir_overlap(&[task1, task2]);

    assert!(
        warnings.is_empty(),
        "no contention when only 1 task targets dir without worktree"
    );
}

#[test]
fn different_dirs_no_warning() {
    let mut task1 = make_task(Some("task1"), &[]);
    task1.dir = Some("src".to_string());
    let mut task2 = make_task(Some("task2"), &[]);
    task2.dir = Some("lib".to_string());

    let warnings = warn_dir_overlap(&[task1, task2]);

    assert!(warnings.is_empty());
}

#[test]
fn defaults_group_parsed() {
    let (config, _) = parse_batch_with_vars(
        "[defaults]\ngroup = \"my-wg\"\n\n[[task]]\nagent = \"codex\"\nprompt = \"do X\"\nworktree = \"a\"\n\n[[task]]\nagent = \"codex\"\nprompt = \"do Y\"\nworktree = \"b\"\n",
        &[],
    );
    assert_eq!(config.defaults.group, Some("my-wg".to_string()));
}

#[test]
fn defaults_group_does_not_override_task_group() {
    let (config, _) = parse_batch_with_vars(
        "[defaults]\ngroup = \"default-wg\"\n\n[[task]]\nagent = \"codex\"\nprompt = \"do X\"\nworktree = \"a\"\ngroup = \"task-wg\"\n\n[[task]]\nagent = \"codex\"\nprompt = \"do Y\"\nworktree = \"b\"\n",
        &[],
    );
    // Task-level group should NOT be overwritten by defaults
    assert_eq!(config.tasks[0].group, Some("task-wg".to_string()));
    // Task without explicit group remains None (assignment happens in cmd/batch.rs)
    assert_eq!(config.tasks[1].group, None);
}

#[test]
fn rejects_unnamed_tasks_sharing_worktree() {
    let file = write_temp(concat!(
        "[[tasks]]\nagent = \"codex\"\nprompt = \"a\"\nworktree = \"feat/shared\"\n",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/shared\""
    ));
    let err = parse_batch_file(file.path()).unwrap_err().to_string();
    assert!(
        err.contains("has no name"),
        "should reject unnamed tasks sharing worktree, got: {err}"
    );
}

#[test]
fn accepts_single_unnamed_task_with_worktree() {
    let file = write_temp(concat!(
        "[[tasks]]\nagent = \"codex\"\nprompt = \"a\"\nworktree = \"feat/solo\"\n",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/other\""
    ));
    assert!(
        parse_batch_file(file.path()).is_ok(),
        "single unnamed task per worktree should be fine"
    );
}

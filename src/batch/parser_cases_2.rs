// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn task_values_override_defaults() {
    let file = write_temp(concat!(
            "[defaults]\nagent = \"gemini\"\ndir = \"src\"\nmodel = \"gpt-5\"\n",
            "worktree_prefix = \"feat\"\nverify = true\nmax_duration_mins = 25\nmax_wait_mins = 10\n",
            "retry = 2\npeer_review = \"gemini\"\nbest_of = 3\nmetric = \"cargo test\"\n",
            "context = [\"src/default.rs\"]\nskills = [\"rust\"]\non_done = \"notify done\"\n",
            "fallback = \"cursor\"\n",
            "env = { DEFAULT_ONLY = \"yes\", SHARED = \"default\" }\n",
            "env_forward = [\"PATH\"]\n",
            "[[tasks]]\nname = \"impl\"\nagent = \"codex\"\nprompt = \"build it\"\n",
            "dir = \"custom\"\nmodel = \"gpt-4\"\nworktree = \"manual/impl\"\n",
            "verify = \"manual\"\nmax_duration_mins = 5\nmax_wait_mins = 2\nretry = 7\npeer_review = \"cursor\"\n",
            "best_of = 5\nmetric = \"just verify\"\ncontext = [\"src/task.rs\"]\n",
            "skills = [\"own\"]\non_done = \"echo done\"\nfallback = \"opencode\"\n",
            "env = { SHARED = \"task\", TASK_ONLY = \"set\" }\n",
            "env_forward = [\"HOME\"]\n"
        ));
    let batch_dir = file.path().parent().unwrap();
    std::fs::create_dir_all(batch_dir.join("custom")).unwrap();
    std::fs::create_dir_all(batch_dir.join("src")).unwrap();
    std::fs::write(batch_dir.join("src/task.rs"), "pub fn task() {}\n").unwrap();
    let cfg = parse_batch_file(file.path()).unwrap();

    let task = &cfg.tasks[0];
    assert_eq!(task.agent, "codex");
    assert_eq!(
        task.dir.as_deref(),
        Some(batch_dir.join("custom").to_string_lossy().as_ref())
    );
    assert_eq!(task.model.as_deref(), Some("gpt-4"));
    assert_eq!(task.worktree.as_deref(), Some("manual/impl"));
    assert_eq!(task.verify.as_deref(), Some("manual"));
    assert_eq!(task.max_duration_mins, Some(5));
    assert_eq!(task.max_wait_mins, Some(2));
    assert_eq!(task.retry, Some(7));
    assert_eq!(task.peer_review.as_deref(), Some("cursor"));
    assert_eq!(task.best_of, Some(5));
    assert_eq!(task.metric.as_deref(), Some("just verify"));
    assert_eq!(
        task.context.as_deref(),
        Some(&[batch_dir.join("src/task.rs").to_string_lossy().into_owned()][..])
    );
    assert_eq!(task.skills.as_deref(), Some(&["own".to_string()][..]));
    assert_eq!(task.on_done.as_deref(), Some("echo done"));
    assert_eq!(task.fallback.as_deref(), Some("opencode"));
    assert_eq!(
        task.env
            .as_ref()
            .and_then(|env| env.get("DEFAULT_ONLY"))
            .map(String::as_str),
        Some("yes")
    );
    assert_eq!(
        task.env
            .as_ref()
            .and_then(|env| env.get("SHARED"))
            .map(String::as_str),
        Some("task")
    );
    assert_eq!(
        task.env
            .as_ref()
            .and_then(|env| env.get("TASK_ONLY"))
            .map(String::as_str),
        Some("set")
    );
    assert_eq!(
        task.env_forward.as_deref(),
        Some(&["PATH".to_string(), "HOME".to_string()][..])
    );
}

#[test]
fn resolve_batch_paths_resolves_relative_dir_from_batch_parent() {
    let temp = tempdir().unwrap();
    let batch_dir = temp.path().join("batches");
    std::fs::create_dir_all(&batch_dir).unwrap();
    std::fs::create_dir_all(batch_dir.join("subdir")).unwrap();
    let batch_path = write_batch_file(
        &batch_dir,
        "tasks.toml",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\ndir = \"subdir\"\n",
    );

    let cfg = parse_batch_file(&batch_path).unwrap();

    assert_eq!(
        cfg.tasks[0].dir.as_deref(),
        Some(batch_dir.join("subdir").to_string_lossy().as_ref())
    );
}

#[test]
fn resolve_batch_paths_leaves_absolute_dir_unchanged() {
    let temp = tempdir().unwrap();
    let batch_dir = temp.path().join("batches");
    std::fs::create_dir_all(&batch_dir).unwrap();
    let absolute_dir = temp.path().join("absolute-dir");
    std::fs::create_dir_all(&absolute_dir).unwrap();
    let batch_path = write_batch_file(
        &batch_dir,
        "tasks.toml",
        &format!(
            "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\ndir = {:?}\n",
            absolute_dir
        ),
    );

    let cfg = parse_batch_file(&batch_path).unwrap();

    assert_eq!(cfg.tasks[0].dir.as_deref(), Some(absolute_dir.to_string_lossy().as_ref()));
}

#[test]
fn resolve_batch_paths_resolves_context_entries() {
    let temp = tempdir().unwrap();
    let batch_dir = temp.path().join("batches");
    std::fs::create_dir_all(&batch_dir).unwrap();
    let absolute_context = temp.path().join("global.md");
    std::fs::write(batch_dir.join("notes.md"), "notes\n").unwrap();
    std::fs::write(&absolute_context, "global\n").unwrap();
    let batch_path = write_batch_file(
        &batch_dir,
        "tasks.toml",
        &format!(
            "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\ncontext = [\"notes.md\", {:?}]\n",
            absolute_context
        ),
    );

    let cfg = parse_batch_file(&batch_path).unwrap();

    assert_eq!(
        cfg.tasks[0].context.as_deref(),
        Some(
            &[
                batch_dir.join("notes.md").to_string_lossy().into_owned(),
                absolute_context.to_string_lossy().into_owned(),
            ][..]
        )
    );
}

#[test]
fn resolve_batch_paths_resolves_dot_relative_to_toml_dir() {
    let temp = tempdir().unwrap();
    let repo_dir = temp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    let batch_path = write_batch_file(
        &repo_dir,
        "tasks.toml",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\ndir = \".\"\n",
    );

    let cfg = parse_batch_file(&batch_path).unwrap();

    assert_eq!(cfg.tasks[0].dir.as_deref(), Some(repo_dir.to_string_lossy().as_ref()));
}

#[test]
fn resolve_batch_paths_fall_back_to_pwd_when_source_path_is_unavailable() {
    let temp = tempdir().unwrap();
    let pwd = temp.path().join("repo");
    std::fs::create_dir_all(pwd.join("project")).unwrap();
    std::fs::write(pwd.join("notes.md"), "notes\n").unwrap();
    let archived_batch = write_batch_file(
        temp.path(),
        "archived.toml",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\ndir = \"project\"\ncontext = [\"notes.md\"]\n",
    );

    let cfg = parse_batch_file_with_vars_and_source(
        &archived_batch,
        &std::collections::HashMap::new(),
        None,
        Some(&pwd),
    )
    .unwrap();

    assert_eq!(
        cfg.tasks[0].dir.as_deref(),
        Some(pwd.join("project").to_string_lossy().as_ref())
    );
    assert_eq!(
        cfg.tasks[0].context.as_deref(),
        Some(&[pwd.join("notes.md").to_string_lossy().into_owned()][..])
    );
}

#[test]
fn resolve_batch_paths_errors_when_relative_dir_cannot_be_resolved() {
    let temp = tempdir().unwrap();
    let batch_dir = temp.path().join("batches");
    let pwd = temp.path().join("repo");
    std::fs::create_dir_all(&batch_dir).unwrap();
    std::fs::create_dir_all(&pwd).unwrap();
    let batch_path = write_batch_file(
        &batch_dir,
        "tasks.toml",
        "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\ndir = \"missing\"\n",
    );

    let err = parse_batch_file_with_vars_and_source(
        &batch_path,
        &std::collections::HashMap::new(),
        Some(&batch_path),
        Some(&pwd),
    )
    .unwrap_err()
    .to_string();

    assert_eq!(
        err,
        format!(
            "dir = 'missing' in batch TOML could not be resolved: tried {} and {} — use an absolute path or place the TOML inside the target repo",
            batch_dir.join("missing").display(),
            pwd.join("missing").display()
        )
    );
}

#[test]
fn empty_defaults_do_not_change_existing_behavior() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\n",
            "[[tasks]]\nagent = \"codex\"\nprompt = \"do something\"\n"
        ))
        .path(),
    )
    .unwrap();

    let task = &cfg.tasks[0];
    assert_eq!(task.agent, "codex");
    assert!(task.dir.is_none());
    assert!(task.verify.is_none());
    assert!(!task.read_only);
    assert!(task.budget.is_none());
}

#[test]
fn rejects_missing_agent_without_defaults() {
    let err = parse_batch_file(write_temp("[[tasks]]\nprompt = \"do something\"\n").path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("aid advise"), "expected advise hint, got: {err}");
}

#[test]
fn rejects_auto_agent() {
    let err = parse_batch_file(
        write_temp("[[tasks]]\nagent = \"auto\"\nprompt = \"do something\"\n").path(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("aid advise"), "expected advise hint, got: {err}");
    assert!(err.contains("removed"), "expected removed message, got: {err}");
}

#[test]
fn rejects_empty_agent_even_with_team() {
    let err = parse_batch_file(
        write_temp("[[tasks]]\nteam = \"dev\"\nprompt = \"do something\"\n").path(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("aid advise"), "expected advise hint, got: {err}");
}

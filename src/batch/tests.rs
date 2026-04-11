// Batch parser tests covering TOML parsing, validation, and defaults resolution.
// Exports: module-local tests only.
// Deps: super::parse_batch_file, super::validate_dag, tempfile::NamedTempFile

use super::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, tempdir};

fn write_temp(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

fn write_batch_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn parse_batch_with_vars(content: &str, cli_vars: &[(&str, &str)]) -> (BatchConfig, String) {
    let vars = cli_vars
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let mut stderr = Vec::new();
    let mut config = toml::from_str::<BatchConfig>(content).unwrap();
    interpolate_batch_config(&mut config, &vars, &mut stderr).unwrap();
    apply_defaults(&mut config.tasks, &config.defaults);
    (config, String::from_utf8(stderr).unwrap())
}

fn make_task(name: Option<&str>, depends_on: &[&str]) -> BatchTask {
    BatchTask {
        id: None,
        name: name.map(str::to_string),
        agent: "codex".to_string(),
        team: None,
        prompt: "prompt".to_string(),
        prompt_file: None,
        dir: None,
        output: None,
        result_file: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        best_of: None,
        max_duration_mins: None,
        max_wait_mins: None,
        retry: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        idle_timeout: None,
        verify: None,
        setup: None,
        judge: None,
        peer_review: None,
        metric: None,
        context: None,
        checklist: None,
        skills: None,
        on_done: None,
        hooks: None,
        depends_on: (!depends_on.is_empty())
            .then(|| depends_on.iter().map(|item| item.to_string()).collect()),
        parent: None,
        context_from: None,
        fallback: None,
        scope: None,
        read_only: false,
        sandbox: false,
        no_skill: false,
        budget: false,
        env: None,
        env_forward: None,
        worktree_link_deps: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    }
}

#[test]
fn parse_valid_batch() {
    let file = write_temp(concat!(
            "[[tasks]]\nagent = \"gemini\"\nprompt = \"research X\"\nworktree = \"feat/x\"\n",
            "[[tasks]]\nagent = \"codex\"\nprompt = \"implement Y\"\ndir = \"src\"\nmodel = \"gpt-4\"\ngroup = \"wg-demo\""
        ));
    let cfg = parse_batch_file(file.path()).unwrap();
    let expected_dir = file.path().parent().unwrap().join("src");

    assert_eq!(cfg.tasks.len(), 2);
    assert_eq!(cfg.tasks[0].agent, "gemini");
    assert_eq!(cfg.tasks[0].worktree, Some("feat/x".into()));
    assert_eq!(
        cfg.tasks[1].dir.as_deref(),
        Some(expected_dir.to_string_lossy().as_ref())
    );
    assert_eq!(cfg.tasks[1].group.as_deref(), Some("wg-demo"));
}

#[test]
fn parse_batch_metadata_fields() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "title = \"My Batch\"\n",
            "description = \"Batch metadata\"\n",
            "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.title.as_deref(), Some("My Batch"));
    assert_eq!(cfg.description.as_deref(), Some("Batch metadata"));
}

#[test]
fn result_file_deserializes_from_batch_toml() {
    let config: BatchConfig = toml::from_str("[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\nresult_file = \"result.md\"\n").unwrap();
    assert_eq!(config.tasks[0].result_file.as_deref(), Some("result.md"));
}

#[test]
fn setup_and_link_deps_defaults_parse_and_task_overrides() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\nagent = \"codex\"\nsetup = \"npm ci\"\nworktree_link_deps = false\n",
            "[[tasks]]\nname = \"defaulted\"\nprompt = \"fix it\"\n",
            "[[tasks]]\nname = \"overridden\"\nprompt = \"ship it\"\nsetup = \"pnpm install\"\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.defaults.setup.as_deref(), Some("npm ci"));
    assert_eq!(cfg.defaults.worktree_link_deps, Some(false));
    assert_eq!(cfg.tasks[0].setup.as_deref(), Some("npm ci"));
    assert_eq!(cfg.tasks[0].worktree_link_deps, Some(false));
    assert_eq!(cfg.tasks[1].setup.as_deref(), Some("pnpm install"));
    assert_eq!(cfg.tasks[1].worktree_link_deps, Some(false));
}

#[test]
fn iterate_fields_work_in_defaults_and_tasks() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\nagent = \"codex\"\niterate = 3\neval = \"cargo test\"\n",
            "eval_feedback_template = \"Round {iteration}/{max_iterations}: {eval_output}\"\n",
            "[[tasks]]\nname = \"defaulted\"\nprompt = \"fix it\"\n",
            "[[tasks]]\nname = \"overridden\"\nprompt = \"ship it\"\niterate = 5\n",
            "eval = \"cargo clippy\"\n",
            "eval_feedback_template = \"Retry {iteration}: {eval_output}\"\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks[0].iterate, Some(3));
    assert_eq!(cfg.tasks[0].eval.as_deref(), Some("cargo test"));
    assert_eq!(
        cfg.tasks[0].eval_feedback_template.as_deref(),
        Some("Round {iteration}/{max_iterations}: {eval_output}")
    );
    assert_eq!(cfg.tasks[1].iterate, Some(5));
    assert_eq!(cfg.tasks[1].eval.as_deref(), Some("cargo clippy"));
    assert_eq!(
        cfg.tasks[1].eval_feedback_template.as_deref(),
        Some("Retry {iteration}: {eval_output}")
    );
}

#[test]
fn parses_batch_with_dependencies() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[[tasks]]\nname = \"foundation\"\nagent = \"codex\"\nprompt = \"shared types\"\n",
            "[[tasks]]\nname = \"feature-a\"\nagent = \"codex\"\nprompt = \"feature a\"\n",
            "depends_on = [\"foundation\"]\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks[0].name.as_deref(), Some("foundation"));
    assert_eq!(
        cfg.tasks[1].depends_on.as_deref(),
        Some(&["foundation".to_string()][..])
    );
}

#[test]
fn context_accepts_string() {
    let toml = r#"
[[tasks]]
prompt = "test"
context = "file.md"
"#;

    let config: BatchConfig = toml::from_str(toml).unwrap();

    assert_eq!(config.tasks[0].context, Some(vec!["file.md".to_string()]));
}

#[test]
fn context_accepts_array() {
    let toml = r#"
[[tasks]]
prompt = "test"
context = ["a.md", "b.md"]
"#;

    let config: BatchConfig = toml::from_str(toml).unwrap();

    assert_eq!(
        config.tasks[0].context,
        Some(vec!["a.md".to_string(), "b.md".to_string()])
    );
}

#[test]
fn rejects_unknown_task_field() {
    let toml = r#"
[[tasks]]
prompt = "test"
promt = "typo"
"#;

    let err = toml::from_str::<BatchConfig>(toml).unwrap_err().to_string();

    assert!(err.contains("unknown field"));
    assert!(err.contains("promt"));
}

#[test]
fn rejects_unknown_defaults_field() {
    let toml = r#"
[defaults]
agentt = "codex"

[[tasks]]
prompt = "test"
"#;

    let err = toml::from_str::<BatchConfig>(toml).unwrap_err().to_string();

    assert!(err.contains("unknown field"));
    assert!(err.contains("agentt"));
}

#[test]
fn applies_defaults_to_tasks() {
    let file = write_temp(concat!(
            "[defaults]\nauto_fallback = true\nagent = \"gemini\"\ndir = \"src\"\nmodel = \"gpt-5\"\n",
            "worktree_prefix = \"feat\"\nverify = true\nmax_duration_mins = 25\nmax_wait_mins = 10\n",
            "retry = 2\npeer_review = \"cursor\"\nbest_of = 3\nmetric = \"cargo test\"\n",
            "context = [\"src/lib.rs\", \"src/main.rs:run\"]\n",
            "skills = [\"rust\", \"cli\"]\non_done = \"notify done\"\nfallback = \"cursor\"\n",
            "read_only = true\nsandbox = true\nno_skill = true\nbudget = true\n",
            "env = { DEFAULT_ONLY = \"yes\", SHARED = \"default\" }\n",
            "env_forward = [\"PATH\"]\n",
            "[[tasks]]\nname = \"impl\"\nprompt = \"build it\"\n"
        ));
    let cfg = parse_batch_file(file.path()).unwrap();
    let batch_dir = file.path().parent().unwrap();

    assert_eq!(cfg.defaults.auto_fallback, Some(true));
    let task = &cfg.tasks[0];
    assert_eq!(task.agent, "gemini");
    assert_eq!(
        task.dir.as_deref(),
        Some(batch_dir.join("src").to_string_lossy().as_ref())
    );
    assert_eq!(task.model.as_deref(), Some("gpt-5"));
    assert_eq!(task.worktree.as_deref(), Some("feat/impl"));
    assert_eq!(task.verify.as_deref(), Some("auto"));
    assert_eq!(task.max_duration_mins, Some(25));
    assert_eq!(task.max_wait_mins, Some(10));
    assert_eq!(task.retry, Some(2));
    assert_eq!(task.peer_review.as_deref(), Some("cursor"));
    assert_eq!(task.best_of, Some(3));
    assert_eq!(task.metric.as_deref(), Some("cargo test"));
    assert_eq!(
        task.context.as_deref(),
        Some(
            &[
                batch_dir.join("src/lib.rs").to_string_lossy().into_owned(),
                batch_dir
                    .join("src/main.rs:run")
                    .to_string_lossy()
                    .into_owned(),
            ][..]
        )
    );
    assert_eq!(
        task.skills.as_deref(),
        Some(&["rust".to_string(), "cli".to_string()][..])
    );
    assert_eq!(task.on_done.as_deref(), Some("notify done"));
    assert_eq!(task.fallback.as_deref(), Some("cursor"));
    assert!(task.read_only);
    assert!(task.sandbox);
    assert!(task.no_skill);
    assert!(task.budget);
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
        Some("default")
    );
    assert_eq!(task.env_forward.as_deref(), Some(&["PATH".to_string()][..]));
}

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
    let cfg = parse_batch_file(file.path()).unwrap();
    let batch_dir = file.path().parent().unwrap();

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
fn resolve_batch_paths_handles_batch_file_in_current_dir() {
    let mut tasks = [BatchTask {
        dir: Some("subdir".to_string()),
        context: Some(vec!["notes.md".to_string()]),
        ..make_task(Some("task"), &[])
    }];

    resolve_batch_paths(&mut tasks, Path::new("tasks.toml"));

    assert_eq!(tasks[0].dir.as_deref(), Some("subdir"));
    assert_eq!(tasks[0].context.as_deref(), Some(&["notes.md".to_string()][..]));
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
    assert!(!task.budget);
}

#[test]
fn rejects_missing_agent_without_defaults() {
    let err = parse_batch_file(write_temp("[[tasks]]\nprompt = \"do something\"\n").path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("missing agent"));
}

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

#[test]
fn judge_true_defaults_to_gemini() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\njudge = true\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks[0].judge.as_deref(), Some("gemini"));
}

#[test]
fn judge_string_uses_specified_agent() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\njudge = \"cursor\"\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks[0].judge.as_deref(), Some("cursor"));
}

#[test]
fn judge_false_is_none() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\njudge = false\n"
        ))
        .path(),
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

#[test]
fn worktree_prefix_generates_worktree_for_unnamed_tasks() {
    let (cfg, _) = parse_batch_with_vars(
        concat!(
            "[defaults]\nworktree_prefix = \"feat\"\nagent = \"codex\"\n",
            "[[task]]\nprompt = \"unnamed task\"\n"
        ),
        &[],
    );
    assert_eq!(
        cfg.tasks[0].worktree.as_deref(),
        Some("feat/task-0"),
        "unnamed task should get index-based worktree"
    );
}

#[test]
fn worktree_prefix_prefers_name_over_index() {
    let (cfg, _) = parse_batch_with_vars(
        concat!(
            "[defaults]\nworktree_prefix = \"feat\"\nagent = \"codex\"\n",
            "[[task]]\nname = \"impl\"\nprompt = \"named task\"\n"
        ),
        &[],
    );
    assert_eq!(
        cfg.tasks[0].worktree.as_deref(),
        Some("feat/impl"),
        "named task should use name, not index"
    );
}

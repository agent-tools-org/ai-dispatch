// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn parse_valid_batch() {
    let file = write_temp(concat!(
            "[[tasks]]\nagent = \"gemini\"\nprompt = \"research X\"\nworktree = \"feat/x\"\n",
            "[[tasks]]\nagent = \"codex\"\nprompt = \"implement Y\"\ndir = \"src\"\nmodel = \"gpt-4\"\ngroup = \"wg-demo\""
        ));
    std::fs::create_dir_all(file.path().parent().unwrap().join("src")).unwrap();
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
fn defaults_accept_repo_root() {
    let config: BatchConfig = toml::from_str(
        "[defaults]\nrepo_root = \"..\"\n[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\n",
    )
    .unwrap();

    assert_eq!(config.defaults.repo_root.as_deref(), Some(".."));
}

#[test]
fn result_file_deserializes_from_batch_toml() {
    let config: BatchConfig = toml::from_str("[[tasks]]\nagent = \"codex\"\nprompt = \"test\"\nresult_file = \"result.md\"\n").unwrap();
    assert_eq!(config.tasks[0].result_file.as_deref(), Some("result.md"));
}

#[test]
fn audit_defaults_and_task_override_parse() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\nagent = \"codex\"\naudit = false\n",
            "[[tasks]]\nname = \"plain\"\nprompt = \"plain\"\n",
            "[[tasks]]\nname = \"audited\"\nprompt = \"audited\"\naudit = true\n"
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.defaults.audit, Some(false));
    assert_eq!(cfg.tasks[0].audit, Some(false));
    assert_eq!(cfg.tasks[1].audit, Some(true));
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
            "read_only = true\nsandbox = true\nno_skill = true\nbudget = \"cheap\"\n",
            "env = { DEFAULT_ONLY = \"yes\", SHARED = \"default\" }\n",
            "env_forward = [\"PATH\"]\n",
            "[[tasks]]\nname = \"impl\"\nprompt = \"build it\"\n"
        ));
    let batch_dir = file.path().parent().unwrap();
    std::fs::create_dir_all(batch_dir.join("src")).unwrap();
    std::fs::write(batch_dir.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    std::fs::write(batch_dir.join("src/main.rs"), "fn run() {}\n").unwrap();
    let cfg = parse_batch_file(file.path()).unwrap();

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
    assert_eq!(task.budget, Some(crate::types::TaskBudget::Cheap));
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

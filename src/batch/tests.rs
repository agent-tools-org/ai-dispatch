// Batch parser tests covering TOML parsing, validation, and defaults resolution.
// Exports: module-local tests only.
// Deps: super::parse_batch_file, super::validate_dag, tempfile::NamedTempFile

use super::*;
use std::io::Write;
use tempfile::NamedTempFile;

fn write_temp(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

fn make_task(name: Option<&str>, depends_on: &[&str]) -> BatchTask {
        BatchTask {
            name: name.map(str::to_string),
            agent: "codex".to_string(),
            team: None,
            prompt: "prompt".to_string(),
            dir: None,
            output: None,
            model: None,
            worktree: None,
            group: None,
            verify: None,
            max_duration_mins: None,
            context: None,
            skills: None,
            hooks: None,
            depends_on: (!depends_on.is_empty())
                .then(|| depends_on.iter().map(|item| item.to_string()).collect()),
            context_from: None,
            fallback: None,
            read_only: false,
            budget: false,
            on_success: None,
            on_fail: None,
            conditional: false,
        }
    }

#[test]
fn parse_valid_batch() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[[task]]\nagent = \"gemini\"\nprompt = \"research X\"\nworktree = \"feat/x\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"implement Y\"\ndir = \"src\"\nmodel = \"gpt-4\"\ngroup = \"wg-demo\""
        ))
        .path(),
    )
    .unwrap();

    assert_eq!(cfg.tasks.len(), 2);
    assert_eq!(cfg.tasks[0].agent, "gemini");
    assert_eq!(cfg.tasks[0].worktree, Some("feat/x".into()));
    assert_eq!(cfg.tasks[1].dir, Some("src".into()));
    assert_eq!(cfg.tasks[1].group.as_deref(), Some("wg-demo"));
}

#[test]
fn parses_batch_with_dependencies() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[[task]]\nname = \"foundation\"\nagent = \"codex\"\nprompt = \"shared types\"\n",
            "[[task]]\nname = \"feature-a\"\nagent = \"codex\"\nprompt = \"feature a\"\n",
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
fn applies_defaults_to_tasks() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\nagent = \"gemini\"\ndir = \"src\"\nmodel = \"gpt-5\"\n",
            "worktree_prefix = \"feat\"\nverify = true\nmax_duration_mins = 25\n",
            "context = [\"src/lib.rs\", \"src/main.rs:run\"]\n",
            "skills = [\"rust\", \"cli\"]\nfallback = \"cursor\"\nread_only = true\nbudget = true\n",
            "[[task]]\nname = \"impl\"\nprompt = \"build it\"\n"
        ))
        .path(),
    )
    .unwrap();

    let task = &cfg.tasks[0];
    assert_eq!(task.agent, "gemini");
    assert_eq!(task.dir.as_deref(), Some("src"));
    assert_eq!(task.model.as_deref(), Some("gpt-5"));
    assert_eq!(task.worktree.as_deref(), Some("feat/impl"));
    assert_eq!(task.verify.as_deref(), Some("auto"));
    assert_eq!(task.max_duration_mins, Some(25));
    assert_eq!(
        task.context.as_deref(),
        Some(&["src/lib.rs".to_string(), "src/main.rs:run".to_string()][..])
    );
    assert_eq!(
        task.skills.as_deref(),
        Some(&["rust".to_string(), "cli".to_string()][..])
    );
    assert_eq!(task.fallback.as_deref(), Some("cursor"));
    assert!(task.read_only);
    assert!(task.budget);
}

#[test]
fn task_values_override_defaults() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\nagent = \"gemini\"\ndir = \"src\"\nmodel = \"gpt-5\"\n",
            "worktree_prefix = \"feat\"\nverify = true\nmax_duration_mins = 25\n",
            "context = [\"src/default.rs\"]\nskills = [\"rust\"]\nfallback = \"cursor\"\n",
            "[[task]]\nname = \"impl\"\nagent = \"codex\"\nprompt = \"build it\"\n",
            "dir = \"custom\"\nmodel = \"gpt-4\"\nworktree = \"manual/impl\"\n",
            "verify = \"manual\"\nmax_duration_mins = 5\n",
            "context = [\"src/task.rs\"]\nskills = [\"own\"]\nfallback = \"opencode\"\n"
        ))
        .path(),
    )
    .unwrap();

    let task = &cfg.tasks[0];
    assert_eq!(task.agent, "codex");
    assert_eq!(task.dir.as_deref(), Some("custom"));
    assert_eq!(task.model.as_deref(), Some("gpt-4"));
    assert_eq!(task.worktree.as_deref(), Some("manual/impl"));
    assert_eq!(task.verify.as_deref(), Some("manual"));
    assert_eq!(task.max_duration_mins, Some(5));
    assert_eq!(
        task.context.as_deref(),
        Some(&["src/task.rs".to_string()][..])
    );
    assert_eq!(task.skills.as_deref(), Some(&["own".to_string()][..]));
    assert_eq!(task.fallback.as_deref(), Some("opencode"));
}

#[test]
fn empty_defaults_do_not_change_existing_behavior() {
    let cfg = parse_batch_file(
        write_temp(concat!(
            "[defaults]\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"do something\"\n"
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
    let err = parse_batch_file(write_temp("[[task]]\nprompt = \"do something\"\n").path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("missing agent"));
}

#[test]
fn rejects_unknown_agent() {
    let file = write_temp("[[task]]\nagent = \"gpt-3\"\nprompt = \"do something\"");
    assert!(
        parse_batch_file(file.path())
            .unwrap_err()
            .to_string()
            .contains("unknown agent")
    );
}

#[test]
fn rejects_duplicate_worktree() {
    let file = write_temp(concat!(
        "[[task]]\nagent = \"gemini\"\nprompt = \"a\"\nworktree = \"feat/x\"\n",
        "[[task]]\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/x\""
    ));
    assert!(
        parse_batch_file(file.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate worktree")
    );
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
        "[[task]]\nagent = \"codex\"\nprompt = \"do something\"\n",
        "fallback = \"unknown-agent\""
    ));
    assert!(
        parse_batch_file(file.path())
            .unwrap_err()
            .to_string()
            .contains("unknown fallback agent")
    );
}

#[test]
fn accepts_valid_fallback_agent() {
    let file = write_temp(concat!(
        "[[task]]\nagent = \"codex\"\nprompt = \"do something\"\n",
        "fallback = \"opencode\""
    ));
    assert!(parse_batch_file(file.path()).is_ok());
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

// CLI parser-to-run configuration coverage for dispatch flags and defaults.
// Exercises the sole conversion used by run_batch::run.

use super::*;
use crate::cli::{Cli, Commands};
use crate::agent::classifier::TaskCategory;
use crate::types::{TaskBudget, TaskDifficulty, TaskEgress, TaskRigor, TaskUrgency};
use clap::Parser;

fn cli_args(flags: &[&str]) -> run_args::RunArgs {
    let cli = Cli::try_parse_from(["aid", "run", "qwen", "say hi"].into_iter()
        .chain(flags.iter().copied())).expect("parse run flags");
    let Some(Commands::Run(args)) = cli.command else { panic!("run command") };
    args
}

fn parsed_args(flags: &[&str]) -> cmd::run::RunArgs {
    run_args(cli_args(flags))
}

fn configured_args() -> cmd::run::RunArgs {
    parsed_args(&[
        "--repo", "repo", "--repo-root", "root", "--dir", "checkout",
        "--output", "answer.txt", "--result-file", "report.md", "--model", "chosen-model",
        "--difficulty", "complex", "--budget", "cheap", "--urgency", "urgent",
        "--rigor", "critical", "--egress", "local", "--kind", "debugging",
        "--worktree", "fix/task", "--team", "dev", "--group", "wg-test", "--verify",
        "--iterate", "2", "--eval", "evaluate", "--eval-feedback-template", "feedback {output}",
        "--judge", "--peer-review", "claude", "--retry", "3",
        "--context", "one.rs", "two.rs", "--checklist", "first", "second",
        "--scope", "src", "tests", "--remote-build", "selected-box", "--no-backup",
        "--context-from", "t-one", "t-two", "--skill", "implementer", "auditor",
        "--template", "template", "--on-done", "finish", "--cascade", "codex,claude",
        "--hook", "done:hook", "--bg", "--dry-run", "--read-only", "--best-of", "2",
        "--metric", "score", "--parent", "t-parent", "--id", "t-existing",
        "--timeout", "601", "--idle-timeout", "121", "--audit", "--no-link-deps",
    ])
}

#[test]
fn cli_run_conversion_preserves_requested_profile_and_paths() {
    let args = configured_args();
    assert_eq!(args.agent_name, "qwen");
    assert_eq!(args.prompt, "say hi");
    assert_eq!(args.repo.as_deref(), Some("repo"));
    assert_eq!(args.repo_root.as_deref(), Some("root"));
    assert_eq!(args.dir.as_deref(), Some("checkout"));
    assert_eq!(args.output.as_deref(), Some("answer.txt"));
    assert_eq!(args.result_file.as_deref(), Some("report.md"));
    assert_eq!(args.model.as_deref(), Some("chosen-model"));
    assert_eq!(args.model_source, ModelSource::UserSupplied);
    assert_eq!(args.declared_difficulty, Some(TaskDifficulty::Complex));
    assert_eq!(args.declared_budget, Some(TaskBudget::Cheap));
    assert_eq!(args.declared_urgency, Some(TaskUrgency::Urgent));
    assert_eq!(args.declared_rigor, Some(TaskRigor::Critical));
    assert_eq!(args.declared_egress, TaskEgress::Local);
    assert_eq!(args.kind, Some(TaskCategory::Debugging));
    assert_eq!(args.worktree.as_deref(), Some("fix/task"));
    assert_eq!(args.team.as_deref(), Some("dev"));
    assert_eq!(args.group.as_deref(), Some("wg-test"));
}

#[test]
fn cli_run_conversion_preserves_lifecycle_flags() {
    let args = configured_args();
    assert_eq!(args.verify.as_deref(), Some("auto"));
    assert_eq!(args.iterate, Some(2));
    assert_eq!(args.eval.as_deref(), Some("evaluate"));
    assert_eq!(args.eval_feedback_template.as_deref(), Some("feedback {output}"));
    assert_eq!(args.judge.as_deref(), Some("gemini"));
    assert_eq!(args.peer_review.as_deref(), Some("claude"));
    assert_eq!(args.retry, 3);
    assert!(args.background && args.dry_run && args.announce && args.read_only);
    assert_eq!(args.best_of, Some(2));
    assert_eq!(args.metric.as_deref(), Some("score"));
    assert_eq!(args.parent_task_id.as_deref(), Some("t-parent"));
    assert_eq!(args.existing_task_id.as_ref().map(TaskId::as_str), Some("t-existing"));
    assert_eq!(args.timeout, Some(601));
    assert_eq!(args.idle_timeout_secs, Some(121));
    assert!(args.audit && args.audit_explicit);
    assert!(!args.no_audit && !args.link_deps);
}

#[test]
fn cli_run_conversion_preserves_extras_and_injected_files() {
    let args = configured_args();
    assert_eq!(args.context, ["one.rs", "two.rs"]);
    assert_eq!(args.checklist, ["first", "second"]);
    assert_eq!(args.scope, ["src", "tests"]);
    assert_eq!(args.remote_build.as_deref(), Some("selected-box"));
    assert!(args.no_backup);
    assert_eq!(args.context_from, ["t-one", "t-two"]);
    assert_eq!(args.skills, ["implementer", "auditor"]);
    assert_eq!(args.template.as_deref(), Some("template"));
    assert_eq!(args.on_done.as_deref(), Some("finish"));
    assert_eq!(args.cascade, ["codex", "claude"]);
    assert_eq!(args.hooks, ["done:hook"]);
}

#[test]
fn cli_run_conversion_keeps_default_model_resolution_and_optional_flags() {
    let args = parsed_args(&[]);
    assert_eq!(args.model, None, "dispatch resolves an unspecified model");
    assert_eq!(args.model_source, ModelSource::AidResolved);
    assert_eq!(args.declared_difficulty, None);
    assert_eq!(args.declared_budget, None);
    assert_eq!(args.declared_urgency, None);
    assert_eq!(args.declared_rigor, None);
    assert_eq!(args.declared_egress, TaskEgress::Any);
    assert_eq!(args.retry, 0);
    assert!(!args.background && !args.dry_run && !args.read_only);
    assert!(args.announce && args.link_deps);
    let args = parsed_args(&["--no-skill", "--no-audit", "--backup", "gdrive:reports", "--sandbox"]);
    assert_eq!(args.skills, [cmd::run::NO_SKILL_SENTINEL]);
    assert!(args.no_audit && args.sandbox);
    assert_eq!(args.backup.as_deref(), Some("gdrive:reports"));
    let args = parsed_args(&["--container", "dev:latest"]);
    assert_eq!(args.container.as_deref(), Some("dev:latest"));
    let args = parsed_args(&["--remote-build"]);
    assert_eq!(args.remote_build.as_deref(), Some("auto"));
}

#[test]
fn cli_run_conversion_preserves_prompt_file_without_reading_files() {
    let cli = Cli::try_parse_from([
        "aid", "run", "qwen", "--prompt-file", "prompt.md", "--no-hint",
        "--checklist", " inline item ", "--checklist-file", "missing-checklist.txt",
    ]).expect("parse run");
    let Some(Commands::Run(args)) = cli.command else { panic!("run command") };
    assert!(args.no_hint);
    let args = run_args(args);
    assert_eq!(args.prompt, "");
    assert_eq!(args.prompt_file.as_deref(), Some("prompt.md"));
    assert_eq!(args.checklist, [" inline item "]);
}

#[test]
fn cli_run_conversion_does_not_drop_any_configurable_field() {
    let defaults = serde_json::to_value(cmd::run::RunArgs::default()).expect("defaults");
    let fixtures = [
        configured_args(),
        parsed_args(&["--no-skill", "--no-audit", "--backup", "gdrive:x", "--sandbox"]),
        parsed_args(&["--container", "dev:latest"]),
        run_args(Cli::try_parse_from(["aid", "run", "qwen", "--prompt-file", "prompt.md"])
            .expect("parse prompt file").command.and_then(|command| match command {
                Commands::Run(args) => Some(args), _ => None,
            }).expect("run command")),
    ].map(|args| serde_json::to_value(args).expect("fixture"));
    let internal = [
        "result_file_required", "base_branch", "setup", "max_duration_mins", "max_task_cost",
        "foreground", "audit_report_mode", "budget", "session_id", "batch_siblings", "env",
        "env_forward", "judge_retry", "timeout_policy", "suppress_nested_repo_warning",
        "force_default_model", "advised_route",
    ];
    for key in internal {
        let default = defaults.get(key).expect("allowlisted field exists");
        assert!(fixtures.iter().all(|fixture| fixture.get(key) == Some(default)),
            "{key} is CLI-configurable; remove it from the allowlist");
    }
    for (key, default) in defaults.as_object().expect("default object") {
        if internal.contains(&key.as_str()) || key == "model_source" { continue; }
        assert!(fixtures.iter().any(|fixture| fixture.get(key) != Some(default)),
            "CLI field {key} is dropped or missing from the fixtures");
    }
}

#[tokio::test]
async fn cli_run_resolves_agent_before_reading_checklist_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(store::Store::open_memory().expect("store"));
    let mut cli = cli_args(&[
        "--difficulty", "moderate", "--budget", "standard", "--urgency", "normal",
        "--rigor", "standard", "--no-hint", "--checklist-file", "missing-checklist.txt",
    ]);
    cli.agent = "auto".into();
    let err = run(store, cli).await.expect_err("removed agent precedes checklist error");
    assert!(err.to_string().contains("auto"), "{err:#}");
    assert!(!err.to_string().contains("checklist"), "{err:#}");
}

#[tokio::test]
async fn cli_run_reads_checklist_file_after_agent_resolution() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(store::Store::open_memory().expect("store"));
    let cli = cli_args(&[
        "--difficulty", "moderate", "--budget", "standard", "--urgency", "normal",
        "--rigor", "standard", "--no-hint", "--checklist-file", "missing-checklist.txt",
    ]);
    let err = run(store, cli).await.expect_err("checklist file must be read");
    assert!(err.to_string().contains("checklist"), "{err:#}");
}

#[tokio::test]
async fn cli_run_dry_run_persists_merged_checklist_prompt_and_budget_mode() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("aid dirs");
    let agents = temp.path().join("agents");
    std::fs::create_dir_all(&agents).expect("agents dir");
    std::fs::write(agents.join("fixture.toml"),
        "[agent]\nid = \"fixture\"\ndisplay_name = \"Fixture\"\ncommand = \"true\"\n")
        .expect("fixture agent");
    let checklist = temp.path().join("checklist.txt");
    std::fs::write(&checklist, "# ignored\nfile item\n\n").expect("checklist");
    let prompt = temp.path().join("prompt.txt");
    std::fs::write(&prompt, "Inspect the requested configuration fields.").expect("prompt");
    let store = Arc::new(store::Store::open_memory().expect("store"));
    for (budget, configured_budget, expected_budget) in [
        ("standard", false, false), ("cheap", false, true), ("standard", true, true),
    ] {
        std::fs::write(crate::paths::config_path(),
            format!("[selection]\nbudget_mode = {configured_budget}\n")).expect("config");
        let cli = Cli::try_parse_from([
            "aid", "run", "fixture", "--prompt-file", prompt.to_str().expect("prompt path"),
            "--checklist", " inline item ", "--checklist-file", checklist.to_str().expect("checklist path"),
            "--difficulty", "moderate", "--budget", budget, "--urgency", "normal",
            "--rigor", "standard", "--no-hint", "--no-skill", "--dry-run",
            "--dir", temp.path().to_str().expect("dir"),
        ]).expect("parse run");
        let Some(Commands::Run(cli)) = cli.command else { panic!("run command") };
        let task_id = run(store.clone(), cli).await.expect("dry-run dispatch");
        let saved = store.get_task_dispatch_args(task_id.as_str()).expect("saved args")
            .expect("persisted dispatch");
        let args: cmd::run::RunArgs = serde_json::from_str(&saved).expect("decode args");
        assert_eq!(args.checklist, ["inline item", "file item"]);
        assert_eq!(args.prompt, "Inspect the requested configuration fields.");
        assert_eq!(args.prompt_file, None);
        assert_eq!(args.budget, expected_budget, "{budget}, config={configured_budget}");
    }
}

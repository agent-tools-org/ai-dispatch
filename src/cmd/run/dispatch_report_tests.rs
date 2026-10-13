// Dispatch report location regressions for host and container launches.
// Checks automatic delivery defaults and preservation of explicit report paths.
// Deps: parent prepare_dispatch_with, AidHomeGuard and in-memory Store.

use super::*;
use crate::cmd::run::read_only_delivery;

#[test]
fn scope_cut_dispatch_creates_fresh_empty_staging_and_saves_its_identity() {
    let home = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let (first, prepared) = prepare(None, false);
    let (second, _) = prepare(None, false);
    let staging = std::path::Path::new(first.artifact_staging.as_ref().unwrap());
    assert_ne!(first.artifact_staging, second.artifact_staging);
    assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
    assert!(!staging.starts_with(first.dir.as_ref().unwrap()));
    assert!(!staging.starts_with(paths::task_dir(prepared.task_id.as_str())));
    for name in ["result.md", "output.md", "transcript.md"] {
        assert!(!paths::task_dir(prepared.task_id.as_str()).join(name).starts_with(staging));
    }
    let saved = RunArgs::from_dispatch_args_json(&first.dispatch_args_json().unwrap()).unwrap();
    assert_eq!(saved.artifact_staging, first.artifact_staging);
}

#[test]
fn scope_cut_dispatch_destination_validation_rejects_every_writable_root() {
    let home = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let (mut args, _) = prepare(None, false);
    let staging = std::path::PathBuf::from(args.artifact_staging.as_ref().unwrap());
    let other = home.path().join("other-root");
    std::fs::create_dir(&other).unwrap();
    let alias = home.path().join("alias");
    std::os::unix::fs::symlink(&staging, &alias).unwrap();
    let roots = vec![staging.clone(), other.clone()];
    for root in [staging, other, alias] {
        for output in [false, true] {
            let destination = root.join("new/final.md").display().to_string();
            args.output = output.then(|| destination.clone());
            args.result_file = (!output).then_some(destination);
            let error = read_only_delivery::validate_destinations(&args, &roots).unwrap_err();
            assert!(error.to_string().contains("Codex-writable root"), "{error}");
            assert!(!root.join("new").exists());
        }
    }
}

#[test]
fn scope_cut_dispatch_rejects_repository_report_and_output_destinations() {
    for output in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let _home = paths::AidHomeGuard::set(home.path());
        let repo = home.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let destination = repo.join("new/final.md").display().to_string();
        let store = Arc::new(Store::open_memory().unwrap());
        let mut args = RunArgs {
            agent_name: "codex".into(), prompt: "Read-only audit".into(), read_only: true,
            dir: Some(repo.display().to_string()),
            output: output.then(|| destination.clone()),
            result_file: (!output).then_some(destination), ..Default::default()
        };
        assert!(prepare_dispatch_with(&store, &mut args, |_| true).is_err());
        assert!(!repo.join("new").exists());
    }
}

#[test]
fn scope_cut_publication_refuses_symlink_staged_report() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let destination = home.path().join("requested.md");
    let (args, _prepared) = prepare(destination.to_str(), false);
    let (_, source) = read_only_delivery::agent_paths(&args, AgentKind::Codex);
    let source = std::path::Path::new(source.as_ref().unwrap());
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    let target = home.path().join("protected.txt");
    std::fs::write(&target, "must not be published").unwrap();
    std::os::unix::fs::symlink(&target, source).unwrap();
    assert!(read_only_delivery::publish(
        &args, AgentKind::Codex, repo.path().to_str(),
    ).is_err());
    assert!(!destination.exists());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "must not be published");
}

#[test]
fn scope_cut_publication_refuses_directory_staged_report() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let destination = home.path().join("requested.md");
    let (args, _prepared) = prepare(destination.to_str(), false);
    let (_, source) = read_only_delivery::agent_paths(&args, AgentKind::Codex);
    std::fs::create_dir_all(source.unwrap()).unwrap();
    assert!(read_only_delivery::publish(
        &args, AgentKind::Codex, repo.path().to_str(),
    ).is_err());
    assert!(!destination.exists());
}

fn prepare(result: Option<&str>, container: bool) -> (RunArgs, PreparedDispatch) {
    let repo = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Read-only audit of the checkout".into(),
        read_only: true,
        dir: Some(repo.path().display().to_string()),
        result_file: result.map(str::to_string),
        container: container.then(|| "test-image".into()),
        ..Default::default()
    };
    let prepared = prepare_dispatch_with(&store, &mut args, |_| true).unwrap();
    (args, prepared)
}

#[test]
fn read_only_host_dispatch_places_automatic_report_under_task_directory() {
    let home = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let (args, prepared) = prepare(None, false);
    let expected = paths::task_dir(prepared.task_id.as_str()).join("result.md");
    assert_eq!(args.result_file.as_deref(), expected.to_str());
    assert_eq!(args.result_file_required, Some(false));
}

#[test]
fn read_only_host_dispatch_preserves_explicit_result_file() {
    let home = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let (args, _) = prepare(Some("../reports/explicit.md"), false);
    assert_eq!(args.result_file.as_deref(), Some("../reports/explicit.md"));
    assert_eq!(args.result_file_required, Some(true));
}

#[test]
fn read_only_container_dispatch_keeps_checkout_relative_automatic_report() {
    let home = tempfile::tempdir().unwrap();
    let _home = paths::AidHomeGuard::set(home.path());
    let (args, prepared) = prepare(None, true);
    assert_eq!(args.result_file, Some(format!("result-{}.md", prepared.task_id)));
}

#[test]
fn fix_round_two_dispatch_rejects_same_report_and_output_destination() {
    for alias in ["same", "parent", "symlink", "traversal", "missing"] {
        let home = tempfile::tempdir().unwrap();
        let _home = paths::AidHomeGuard::set(home.path());
        let directory = home.path().join("delivery");
        std::fs::create_dir(&directory).unwrap();
        let output = directory.join("final.md");
        std::fs::write(&output, "original\n").unwrap();
        let report = match alias {
            "parent" => {
                std::os::unix::fs::symlink(&directory, home.path().join("alias")).unwrap();
                home.path().join("alias/final.md")
            }
            "symlink" => {
                std::os::unix::fs::symlink(&output, directory.join("alias.md")).unwrap();
                directory.join("alias.md")
            }
            "traversal" => directory.join("missing/../final.md"),
            "missing" => directory.join("new/final.md"),
            _ => output.clone(),
        };
        let store = Arc::new(Store::open_memory().unwrap());
        let mut args = RunArgs {
            agent_name: "codex".into(),
            prompt: "Read-only audit of the checkout".into(),
            read_only: true,
            dir: Some(directory.display().to_string()),
            output: Some(if alias == "missing" { report.clone() } else { output.clone() }
                .display().to_string()),
            result_file: Some(report.display().to_string()),
            existing_task_id: Some(TaskId("t-duplicate-delivery".into())),
            ..Default::default()
        };
        let error = match prepare_dispatch_with(&store, &mut args, |_| true) {
            Ok(_) => panic!("duplicate destinations must be rejected: {alias}"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("--result-file") && error.contains("-o")
            && error.contains("same destination"), "{error}");
        assert!(store.get_task("t-duplicate-delivery").unwrap().is_none());
        assert_eq!(std::fs::read_to_string(output).unwrap(), "original\n");
    }
}

// Dispatch report location regressions for host and container launches.
// Checks automatic delivery defaults and preservation of explicit report paths.
// Deps: parent prepare_dispatch_with, AidHomeGuard and in-memory Store.

use super::*;

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
    let (args, _) = prepare(Some("reports/explicit.md"), false);
    assert_eq!(args.result_file.as_deref(), Some("reports/explicit.md"));
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

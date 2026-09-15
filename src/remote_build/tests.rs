// Remote build configuration, persistence, prompt and verification regression tests.
// Exercises CLI parsing and fake selection without contacting build boxes.
// Deps: clap, tempfile, in-memory Store and sibling shim fixture helpers.
use super::*;
use clap::Parser;
use super::shim_tests::executable;

#[test]
fn cli_optional_box_and_conflicts() {
    for (flags, expected) in [(vec!["--remote-build"], "auto"), (vec!["--remote-build", "auto"], "auto"), (vec!["--remote-build", "box-one"], "box-one")] {
        let cli = crate::cli::Cli::try_parse_from([vec!["aid", "run", "codex", "task"], flags].concat()).expect("parse");
        let Some(crate::cli::Commands::Run(args)) = cli.command else { panic!("run"); };
        assert_eq!(args.run_extras.remote_build.as_deref(), Some(expected));
    }
    for flags in [vec!["--sandbox"], vec!["--container", "image"]] {
        let result = crate::cli::Cli::try_parse_from([vec!["aid", "run", "codex", "task", "--remote-build=auto"], flags].concat());
        assert!(result.is_err());
    }
}

#[test]
fn selection_uses_explicit_name_or_one_pick_and_retains_stderr() {
    let temp = tempfile::tempdir().expect("temp");
    let rbox = temp.path().join("rbox");
    executable(&rbox, "#!/bin/bash\n[[ \"$*\" == \"pick --role rust-build\"* ]] || exit 9\necho picked-box\n");
    assert_eq!(resolve_with("auto", Path::new(""), &mut Command::new(&rbox)).expect("pick"), "picked-box");
    assert_eq!(resolve_with("literal name", Path::new(""), &mut Command::new("missing-command")).expect("explicit"), "literal name");
    executable(&rbox, "#!/bin/bash\necho 'none free: all rust builders busy' >&2\nexit 1\n");
    let error = resolve_with("auto", Path::new(""), &mut Command::new(&rbox)).expect_err("busy");
    assert!(error.to_string().contains("none free: all rust builders busy"));
    executable(&rbox, "#!/bin/bash\nexit 0\n");
    assert!(resolve_with("auto", Path::new(""), &mut Command::new(&rbox)).is_err());
}

fn stored_task() -> (Store, TaskId, RunArgs) {
    let store = Store::open_memory().expect("store");
    store.db().execute("INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES ('t-remote', 'codex', 'task', 'pending', '2026-09-12T00:00:00Z')", []).expect("task");
    let args = RunArgs { remote_build: Some("chosen-box".into()), ..Default::default() };
    store.update_task_dispatch_args("t-remote", &args.dispatch_args_json().expect("serialize")).expect("persist");
    (store, TaskId("t-remote".into()), args)
}

#[test]
fn stored_box_is_reusable_and_visible_in_events_and_show() {
    let (store, task_id, args) = stored_task();
    record(&store, &task_id, &args).expect("event");
    assert_eq!(saved_box(&store, task_id.as_str()).expect("saved"), args.remote_build);
    let restored = RunArgs::saved_for_task(&store, task_id.as_str()).expect("load").expect("args");
    assert_eq!(restored.remote_build.as_deref(), Some("chosen-box"));
    let events = store.get_events(task_id.as_str()).expect("events");
    assert_eq!(events[0].metadata.as_ref().expect("metadata")["remote_build"], "chosen-box");
    let text = crate::cmd::show::audit_text(&std::sync::Arc::new(store), task_id.as_str()).expect("show");
    assert!(text.contains("Remote build box: chosen-box"), "{text}");
}

#[test]
fn prompt_is_active_only_when_resolved() {
    let mut prompt = "Task".to_string();
    append_prompt(&mut prompt, None);
    assert_eq!(prompt, "Task");
    append_prompt(&mut prompt, Some("chosen-box"));
    assert!(prompt.contains("remote box chosen-box through a PATH shim"));
    assert!(prompt.contains("`cargo fmt` stays local"));
}

#[test]
fn task_command_overrides_ambient_box_and_installs_shim() {
    let (store, id, _) = stored_task();
    let home = tempfile::tempdir().expect("home");
    let mut cmd = Command::new("agent");
    cmd.env("AID_BUILD_BOX", "operator-box");
    configure_task(&store, id.as_str(), &mut cmd, home.path()).expect("configure");
    let env = cmd.get_envs().collect::<std::collections::HashMap<_, _>>();
    assert_eq!(env[std::ffi::OsStr::new("AID_BUILD_BOX")], Some(std::ffi::OsStr::new("chosen-box")));
    assert!(home.path().join(".aid-build-shims/cargo").is_file());
}

#[test]
fn verify_receives_stored_box() {
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(&temp.path().join("verify"), "#!/bin/bash\nprintf '%s' \"$AID_BUILD_BOX\"\n");
    let result = verify(&store, id.as_str(), temp.path(), Some("./verify"), None, None).expect("verify");
    assert!(result.success);
    assert_eq!(result.output, "chosen-box");
}

#[test]
fn project_and_batch_accept_remote_build_defaults_and_overrides() {
    let config: crate::project::ProjectConfig = toml::from_str("id = 'project'\nremote_build = 'auto'").expect("project");
    assert_eq!(config.remote_build.as_deref(), Some("auto"));
    let temp = tempfile::tempdir().expect("temp");
    let file = temp.path().join("batch.toml");
    std::fs::write(&file, "[defaults]\nremote_build = 'auto'\n[[tasks]]\nagent = 'codex'\nprompt = 'one'\n[[tasks]]\nagent = 'codex'\nprompt = 'two'\nremote_build = 'box-two'\n").expect("batch");
    let batch = crate::batch::parse_batch_file(&file).expect("parse batch");
    assert_eq!(batch.tasks[0].remote_build.as_deref(), Some("auto"));
    assert_eq!(batch.tasks[1].remote_build.as_deref(), Some("box-two"));
}

#[test]
fn shim_installation_never_follows_operator_aid_symlink() {
    let temp = tempfile::tempdir().expect("temp");
    let operator = temp.path().join("operator-aid");
    let home = temp.path().join("home");
    std::fs::create_dir(&operator).expect("operator");
    std::fs::create_dir(&home).expect("home");
    std::os::unix::fs::symlink(&operator, home.join(".aid")).expect("symlink");
    configure(&mut Command::new("agent"), &home, "box").expect("configure");
    assert!(home.join(".aid-build-shims/cargo").is_file());
    assert_eq!(std::fs::read_dir(&operator).expect("operator entries").count(), 0);
}

#[test]
fn resolve_disabled_is_noop_and_incompatible_is_rejected() {
    let mut args = RunArgs::default();
    resolve(&mut args).expect("disabled");
    args.remote_build = Some("auto".into());
    args.sandbox = true;
    assert!(resolve(&mut args).expect_err("conflict").to_string().contains("conflicts"));
}

#[test]
fn project_auto_default_degrades_to_local_without_rbox() {
    assert_eq!(project_default_with(Some("auto"), false), None);
    assert_eq!(project_default_with(Some("auto"), true).as_deref(), Some("auto"));
    assert_eq!(project_default_with(Some("named-box"), false).as_deref(), Some("named-box"));
    assert_eq!(project_default_with(None, false), None);
}

#[test]
fn explicit_auto_still_requires_rbox() {
    let mut args = RunArgs { remote_build: Some("auto".into()), ..Default::default() };
    let error = resolve_in(&mut args, false, &mut Command::new("rbox")).expect_err("rbox missing");
    assert!(error.to_string().contains("requires rbox on PATH"), "{error}");
    args.remote_build = Some("named-box".into());
    let error = resolve_in(&mut args, false, &mut Command::new("rbox")).expect_err("rbox missing");
    assert!(error.to_string().contains("requires rbox on PATH"), "{error}");
}

#[test]
fn pick_passes_repo_and_exclude_to_rbox() {
    let temp = tempfile::tempdir().expect("temp");
    let rbox = temp.path().join("rbox");
    let argv = temp.path().join("argv");
    executable(&rbox, &format!("#!/bin/bash\nprintf '%s\\n' \"$@\" > '{}'\necho second-box\n", argv.display()));
    let picked = pick(&mut Command::new(&rbox), Path::new("/repos/wt"), Some("grok-bot-chief")).expect("pick");
    assert_eq!(picked, "second-box");
    assert_eq!(std::fs::read_to_string(&argv).expect("argv"), "pick\n--role\nrust-build\n--repo\n/repos/wt\n--exclude\ngrok-bot-chief\n");
    let mut args = RunArgs { remote_build: Some("auto".into()), dir: Some("/repos/main".into()), ..Default::default() };
    resolve_in(&mut args, true, &mut Command::new(&rbox)).expect("resolve");
    assert_eq!(args.remote_build.as_deref(), Some("second-box"));
    assert_eq!(std::fs::read_to_string(&argv).expect("argv"), "pick\n--role\nrust-build\n--repo\n/repos/main\n");
}

fn refusing_verify(dir: &Path, refusing: &str) {
    executable(&dir.join("verify"), &format!("#!/bin/bash\ncase \"$AID_BUILD_BOX\" in {refusing}) echo \"rbox: $AID_BUILD_BOX 9.9 GiB free; requires 10 GiB\" >&2; exit 69 ;; esac\necho \"ran on $AID_BUILD_BOX\"\n"));
}

#[test]
fn refused_verify_repicks_once_persists_box_and_reruns() {
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    refusing_verify(temp.path(), "chosen-box");
    let rbox = temp.path().join("rbox");
    let argv = temp.path().join("argv");
    executable(&rbox, &format!("#!/bin/bash\nprintf '%s\\n' \"$@\" > '{}'\necho second-box\n", argv.display()));
    let result = verify_with(&store, id.as_str(), temp.path(), Some("./verify"), None, None, &mut Command::new(&rbox)).expect("verify");
    assert!(result.success, "{}", result.output);
    assert_eq!(result.output, "ran on second-box\n");
    assert_eq!(std::fs::read_to_string(&argv).expect("argv"), format!("pick\n--role\nrust-build\n--repo\n{}\n--exclude\nchosen-box\n", temp.path().display()));
    assert_eq!(saved_box(&store, id.as_str()).expect("saved").as_deref(), Some("second-box"));
    let events = store.get_events(id.as_str()).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, "Remote build box: chosen-box refused admission (disk); re-picked second-box");
    assert_eq!(events[0].metadata.as_ref().expect("metadata")["remote_build"], "second-box");
}

#[test]
fn second_refusal_or_failed_repick_is_infrastructure_failure_naming_the_box() {
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    refusing_verify(temp.path(), "chosen-box|second-box");
    let rbox = temp.path().join("rbox");
    executable(&rbox, "#!/bin/bash\necho second-box\n");
    let error = verify_with(&store, id.as_str(), temp.path(), Some("./verify"), None, None, &mut Command::new(&rbox)).expect_err("both refused");
    assert_eq!(error.to_string(), "Remote build box second-box refused admission after re-pick from chosen-box (rbox: second-box 9.9 GiB free; requires 10 GiB)");
    assert_eq!(saved_box(&store, id.as_str()).expect("saved").as_deref(), Some("second-box"));
    
    let (store2, id2, _) = stored_task();
    executable(&rbox, "#!/bin/bash\necho 'none free: all rust builders busy' >&2\nexit 1\n");
    let error = verify_with(&store2, id2.as_str(), temp.path(), Some("./verify"), None, None, &mut Command::new(&rbox)).expect_err("re-pick failed");
    assert_eq!(error.to_string(), "Remote build box chosen-box refused admission (rbox: chosen-box 9.9 GiB free; requires 10 GiB); re-pick failed: Remote build box selection failed: none free: all rust builders busy");
    
}

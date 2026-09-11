// Regression coverage for prepared build targets, grants, and task scratch.
// Tests launch helpers using isolated filesystems and command inspection.
// Deps: parent scratch helpers, tempfile, and CargoTargetDirGuard.

use super::*;
use crate::agent::{Agent, CommandContext, RunOpts};

fn opts(dir: &Path) -> RunOpts {
    RunOpts {
        dir: Some(dir.to_string_lossy().into_owned()), output: None, result_file: None,
        model: None, budget: false, read_only: false, sandbox: false,
        context_files: vec![], session_id: None, env: None, env_forward: None,
    }
}

#[test]
fn non_worktree_rust_task_creates_base_target() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let target_guard = crate::test_env::CargoTargetDirGuard::set(&root);
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname = 'scratch'\n").unwrap();
    let target = crate::agent::rust_build_cache_target_dir(temp.path().to_str(), None).unwrap();
    assert_eq!(Path::new(&target), root.join("_base"));
    assert!(!Path::new(&target).exists());

    prepare_cargo_target(Some(&target)).unwrap();

    assert!(Path::new(&target).is_dir());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    drop(target_guard);
}

#[test]
fn regular_checkout_grants_git_target_and_temp() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let target = temp.path().join("cache/_base");
    let scratch = create_temp_dir(temp.path()).unwrap();
    let prepared_target = prepare_cargo_target(target.to_str()).unwrap();
    let (writable_roots, warnings) = prepare_launch_roots(
        AgentKind::Codex, repo.to_str(), prepared_target.as_deref(), &scratch, &TaskId("t-roots".into()),
    ).unwrap();
    assert!(warnings.is_empty());
    let cmd = crate::agent::codex::CodexAgent.build_command_with_context(
        "check", &opts(&repo), CommandContext {
            durable_codex_home: false,
            writable_roots,
        },
    ).unwrap();
    let config = cmd.get_args().find_map(|arg| arg.to_str()
        .filter(|arg| arg.starts_with("sandbox_workspace_write.writable_roots="))).unwrap();
    let parsed: toml::Value = config.parse().unwrap();
    let roots = parsed["sandbox_workspace_write"]["writable_roots"].as_array().unwrap();
    for path in [repo.join(".git"), target, scratch] {
        let path = path.canonicalize().unwrap();
        assert!(roots.contains(&toml::Value::String(path.to_str().unwrap().into())));
    }
    assert_eq!(roots.len(), 3);
}

#[test]
fn codex_capability_command_does_not_grant_caller_directories() {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("AID_PREFLIGHT_UNIT_CHILD").is_none() {
        let temp = tempfile::tempdir().unwrap();
        let git = temp.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o555)).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "agent::scratch::tests::codex_capability_command_does_not_grant_caller_directories"])
            .env("AID_PREFLIGHT_UNIT_CHILD", "1").current_dir(temp.path()).output().unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        assert_eq!(fs::read_dir(&git).unwrap().count(), 0);
        return;
    }
    let mut opts = opts(Path::new("."));
    opts.dir = None;
    let cmd = crate::agent::codex::CodexAgent.build_command("check capabilities", &opts).unwrap();
    assert!(!cmd.get_args().any(|arg| arg.to_string_lossy()
        .starts_with("sandbox_workspace_write.writable_roots=")));
}

#[test]
fn codex_launch_without_explicit_directory_still_grants_scratch() {
    let temp = tempfile::tempdir().unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    let mut opts = opts(temp.path());
    opts.dir = None;
    let cmd = crate::agent::codex::CodexAgent.build_command_with_context(
        "check", &opts, CommandContext {
            durable_codex_home: false, writable_roots: vec![scratch.clone()],
        },
    ).unwrap();
    assert!(cmd.get_args().any(|arg| arg.to_string_lossy()
        .contains(scratch.to_str().unwrap())));
}

#[test]
fn probe_refuses_read_only_directory() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = probe_directory(temp.path());
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let error = result.unwrap_err();
    assert!(error.to_string().contains(temp.path().to_str().unwrap()));
    assert!(error.to_string().contains("cannot write to granted directory"));
}

#[test]
fn target_creation_failure_names_directory() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("blocked");
    fs::write(&target, "file").unwrap();
    let error = prepare_cargo_target(target.to_str()).unwrap_err();
    assert!(error.to_string().contains(target.to_str().unwrap()));
}

#[test]
fn scratch_is_private_even_when_home_contains_temp_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    std::os::unix::fs::symlink(temp.path(), home.join("tmp")).unwrap();
    let first = create_temp_dir(&home).unwrap();
    let second = create_temp_dir(&home).unwrap();
    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(home.canonicalize().unwrap().as_path()));
    assert!(!fs::symlink_metadata(first).unwrap().file_type().is_symlink());
}

#[test]
fn copilot_grants_checkout_metadata_and_both_scratch_paths() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    let target = temp.path().join("cache/_base");
    let opts = opts(&repo);
    let prepared_target = prepare_cargo_target(target.to_str()).unwrap();
    let (writable_roots, warnings) = prepare_launch_roots(
        AgentKind::Copilot, repo.to_str(), prepared_target.as_deref(), &scratch, &TaskId("t-copilot".into()),
    ).unwrap();
    assert!(warnings.is_empty());
    let cmd = crate::agent::copilot::CopilotAgent.build_command_with_context(
        "check", &opts, CommandContext { durable_codex_home: false, writable_roots },
    ).unwrap();
    let args = cmd.get_args().collect::<Vec<_>>();
    for path in [repo.join(".git"), target, scratch] {
        let path = path.canonicalize().unwrap();
        assert!(args.windows(2).any(|pair| pair[0] == "--add-dir" && pair[1] == path));
    }
}

#[test]
fn probe_leaves_existing_contents_untouched() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("keep"), "unchanged").unwrap();
    probe_directory(temp.path()).unwrap();
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    assert_eq!(fs::read_to_string(temp.path().join("keep")).unwrap(), "unchanged");
}

#[test]
fn command_building_does_not_create_or_probe_supplied_roots() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("cache/_base");
    crate::agent::codex::CodexAgent.build_command_with_context(
        "check", &opts(temp.path()), CommandContext {
            durable_codex_home: false, writable_roots: vec![target.clone()],
        },
    ).unwrap();
    assert!(!target.exists());
}

#[test]
fn resumed_codex_grants_temp_via_supported_config_flag() {
    let temp = tempfile::tempdir().unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    let mut opts = opts(temp.path());
    opts.session_id = Some("saved-session".into());
    let cmd = crate::agent::codex::CodexAgent.build_command_with_context(
        "check", &opts, CommandContext {
            durable_codex_home: false, writable_roots: vec![scratch.clone()],
        },
    ).unwrap();
    let args = cmd.get_args().collect::<Vec<_>>();
    assert_eq!(&args[..2], ["exec", "resume"]);
    assert!(!args.contains(&std::ffi::OsStr::new("--add-dir")));
    assert!(args.iter().any(|arg| arg.to_str().unwrap().contains(scratch.to_str().unwrap())));
}

#[test]
fn symlinked_scratch_uses_identical_grants_and_env_paths() {
    let temp = tempfile::tempdir().unwrap();
    let actual = temp.path().join("actual");
    fs::create_dir(&actual).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    let target = prepare_cargo_target(alias.join("_base").to_str()).unwrap().unwrap();
    let scratch = create_temp_dir(&alias).unwrap();
    let mut cmd = Command::new("codex");
    cmd.env("TMPDIR", &scratch).env("CARGO_TARGET_DIR", &target);
    grant_codex_roots(&mut cmd, &[PathBuf::from(&target), scratch.clone()]).unwrap();
    for path in [PathBuf::from(target), scratch] {
        assert!(path.starts_with(actual.canonicalize().unwrap()));
        assert!(cmd.get_args().any(|arg| arg.to_str().unwrap().contains(path.to_str().unwrap())));
        assert!(cmd.get_envs().any(|(_, value)| value == Some(path.as_os_str())));
    }
}

#[test]
fn read_only_git_metadata_is_omitted_with_a_named_warning() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let git = temp.path().join(".git");
    fs::create_dir(&git).unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o555)).unwrap();
    let result = prepare_launch_roots(
        AgentKind::Codex, temp.path().to_str(), None, &scratch, &TaskId("t-read-only".into()),
    );
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    let (roots, warnings) = result.unwrap();
    assert_eq!(roots, vec![scratch]);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].detail.contains(git.to_str().unwrap()));
    assert!(warnings[0].detail.contains("directory is read-only"));
    assert_eq!(fs::read_dir(&git).unwrap().count(), 0);
}

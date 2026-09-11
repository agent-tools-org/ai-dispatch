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
    let cmd = crate::agent::codex::CodexAgent.build_command_with_context(
        "check", &opts(&repo), CommandContext {
            durable_codex_home: false,
            cargo_target_dir: Some(target.to_string_lossy().into_owned()),
            temp_dir: Some(scratch.clone()),
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
    let mut cmd = crate::agent::copilot::CopilotAgent.build_command("check", &opts).unwrap();
    cmd.env("TMPDIR", &scratch);
    grant_launch_dirs(&mut cmd, AgentKind::Copilot, &opts, target.to_str()).unwrap();
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
fn sandbox_mounts_target_and_temp_at_their_granted_paths() {
    let temp = tempfile::tempdir().unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    let target = temp.path().join("cache/_base");
    prepare_cargo_target(target.to_str()).unwrap();
    let mut cmd = Command::new("codex");
    cmd.env("TMPDIR", &scratch).env("CARGO_TARGET_DIR", &target);
    let wrapped = crate::sandbox::wrap_command(&cmd, "t-scratch", AgentKind::Codex, false);
    let args = wrapped.get_args().collect::<Vec<_>>();
    for path in [scratch, target] {
        let mount = format!("{}:{}", path.display(), path.display());
        assert!(args.windows(2).any(|pair| pair[0] == "-v" && pair[1] == mount.as_str()));
    }
}

#[test]
fn container_probe_keeps_paths_out_of_shell_code() {
    let path = Path::new("/task/home/with space/$(untrusted)");
    let cmd = container_probe_command("aid-dev-scratch", path);
    let args = cmd.get_args().collect::<Vec<_>>();
    assert_eq!(&args[..4], ["exec", "aid-dev-scratch", "sh", "-c"]);
    assert!(args[4].to_str().unwrap().contains("mkdir -p"));
    assert!(args[4].to_str().unwrap().contains("&& rm"));
    assert_eq!(args[6], path);
    assert!(!args[4].to_str().unwrap().contains("untrusted"));
}

#[test]
fn resumed_codex_grants_temp_via_supported_config_flag() {
    let temp = tempfile::tempdir().unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    let mut opts = opts(temp.path());
    opts.session_id = Some("saved-session".into());
    let cmd = crate::agent::codex::CodexAgent.build_command_with_context(
        "check", &opts, CommandContext {
            durable_codex_home: false, cargo_target_dir: None, temp_dir: Some(scratch.clone()),
        },
    ).unwrap();
    let args = cmd.get_args().collect::<Vec<_>>();
    assert_eq!(&args[..2], ["exec", "resume"]);
    assert!(!args.contains(&std::ffi::OsStr::new("--add-dir")));
    assert!(args.iter().any(|arg| arg.to_str().unwrap().contains(scratch.to_str().unwrap())));
}

#[test]
fn symlinked_scratch_uses_identical_grants_env_and_container_paths() {
    let temp = tempfile::tempdir().unwrap();
    let actual = temp.path().join("actual");
    fs::create_dir(&actual).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&actual, &alias).unwrap();
    let target = prepare_cargo_target(alias.join("_base").to_str()).unwrap().unwrap();
    let scratch = create_temp_dir(&alias).unwrap();
    let mut cmd = Command::new("codex");
    cmd.env("TMPDIR", &scratch).env("CARGO_TARGET_DIR", &target);
    grant_codex_roots(&mut cmd, temp.path().to_str(), Some(Path::new(&target)), Some(&scratch)).unwrap();
    let mut wrapped = Command::new("container");
    mount_scratch_dirs(&mut wrapped, &cmd);
    for path in command_scratch_dirs(&cmd) {
        assert!(path.starts_with(actual.canonicalize().unwrap()));
        let mount = format!("{}:{}", path.display(), path.display());
        assert!(wrapped.get_args().any(|arg| arg == mount.as_str()));
        assert!(cmd.get_args().any(|arg| arg.to_str().unwrap().contains(path.to_str().unwrap())));
        assert_eq!(container_probe_command("dev", &path).get_args().nth(6).unwrap(), path);
    }
}

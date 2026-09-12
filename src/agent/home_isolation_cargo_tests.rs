// Regression coverage for private Cargo homes and login-shell PATH precedence.
// Exports tests only; depends on HOME isolation, remote shim setup, and shell fixtures.
use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture_home(root: &Path) -> PathBuf {
    let real = root.join("real-home");
    let cargo = real.join(".cargo");
    fs::create_dir_all(cargo.join("bin")).expect("bin");
    for name in ["cargo", "rustup", "cargo-clippy"] {
        let path = cargo.join("bin").join(name);
        fs::write(&path, "#!/bin/sh\nprintf 'real cargo %s\\n' \"$*\"\n").expect("tool");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("executable");
    }
    symlink("rustup", cargo.join("bin/rustc")).expect("rustc link");
    for name in ["registry", "git"] {
        fs::create_dir(cargo.join(name)).expect("cache");
    }
    for name in ["config.toml", "credentials.toml"] {
        fs::write(cargo.join(name), "# fixture\n").expect("config");
    }
    fs::write(cargo.join("env"), "export PATH=\"$HOME/.cargo/bin:$PATH\"\n").expect("env");
    for name in [".profile", ".zshenv"] {
        fs::write(real.join(name), ". \"$HOME/.cargo/env\"\n").expect("profile");
    }
    real
}

#[test]
fn remote_home_materializes_cargo_and_plain_home_keeps_symlink() {
    let temp = tempfile::tempdir().expect("temp");
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let real = fixture_home(temp.path());
    let guard = IsolatedHomeGuard::create_from_home_with_remote_build(Some(&real), None, true).expect("home");
    let cargo = guard.path().join(".cargo");
    for dir in [&cargo, &cargo.join("bin")] {
        let metadata = fs::symlink_metadata(dir).expect("directory");
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    }
    let shim = cargo.join("bin/cargo");
    assert!(!fs::symlink_metadata(&shim).expect("shim").file_type().is_symlink());
    assert_eq!(fs::read_to_string(&shim).expect("shim content"), include_str!("../remote_build/cargo.sh"));
    assert_ne!(fs::metadata(&shim).expect("mode").permissions().mode() & 0o111, 0);
    for entry in ["bin/rustup", "bin/rustc", "bin/cargo-clippy", "env", "config.toml", "credentials.toml", "registry", "git"] {
        assert_eq!(fs::read_link(cargo.join(entry)).expect("passthrough link"), real.join(".cargo").join(entry));
    }
    drop(guard);
    assert_eq!(fs::read_link(real.join(".cargo/bin/rustc")).expect("operator link"), PathBuf::from("rustup"));
    let plain = IsolatedHomeGuard::create_from_home(Some(&real), None).expect("plain home");
    assert_eq!(fs::read_link(plain.path().join(".cargo")).expect("plain link"), real.join(".cargo"));
}

fn shell_command(guard: &IsolatedHomeGuard, real: &Path, shell: &str, script: &str) -> Command {
    let store = crate::store::Store::open_memory().expect("store");
    store.db().execute("INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES ('t-home-cargo', 'codex', 'task', 'pending', '2026-09-12T00:00:00Z')", []).expect("task");
    store.update_task_dispatch_args("t-home-cargo", r#"{"remote_build":"fixture-box"}"#).expect("args");
    let mut command = Command::new(shell);
    command.args(["-lc", script]).env("HOME", guard.path())
        .env("PATH", format!("{}:/usr/bin:/bin", real.join(".cargo/bin").display()))
        .env_remove("ZDOTDIR").env_remove("BASH_ENV").env_remove("ENV");
    guard.apply_toolchain_env(&mut command);
    crate::remote_build::configure_task(&store, "t-home-cargo", &mut command, guard.path()).expect("shim PATH");
    command
}

fn assert_login_shell_keeps_shim(shell: &str) {
    let temp = tempfile::tempdir().expect("temp");
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let real = fixture_home(temp.path());
    let guard = IsolatedHomeGuard::create_from_home_with_remote_build(Some(&real), None, true).expect("home");
    let output = shell_command(&guard, &real, shell, "command -v cargo").output().expect("login shell");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), guard.path().join(".cargo/bin/cargo").to_string_lossy());
    let cargo_path = guard.path().join(".cargo");
    assert!(!fs::symlink_metadata(cargo_path).expect("private Cargo").file_type().is_symlink());
}

#[test]
fn bash_login_shell_resolves_private_cargo_shim() {
    assert_login_shell_keeps_shim("/bin/bash");
}

#[test]
fn zsh_login_shell_resolves_private_cargo_shim_when_available() {
    if !Path::new("/bin/zsh").is_file() {
        eprintln!("zsh login-shell regression skipped: /bin/zsh is unavailable");
        return;
    }
    assert_login_shell_keeps_shim("/bin/zsh");
}

#[test]
fn both_cargo_shims_preserve_local_passthrough_without_recursion() {
    let temp = tempfile::tempdir().expect("temp");
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let real = fixture_home(temp.path());
    let guard = IsolatedHomeGuard::create_from_home_with_remote_build(Some(&real), None, true).expect("home");
    let output = shell_command(&guard, &real, "/bin/bash",
        r#"PATH="$HOME/.cargo/bin:$HOME/.aid-build-shims:$CARGO_HOME/bin:/usr/bin:/bin" cargo fmt --check"#)
        .output().expect("local Cargo passthrough");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "real cargo fmt --check\n");
}

#[test]
fn remote_constructor_keeps_host_toolchain_environment() {
    let temp = tempfile::tempdir().expect("temp");
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let real = resolve_real_home().expect("real home");
    let guard = IsolatedHomeGuard::create_with_remote_build(None, true).expect("remote home");
    let mut command = Command::new("cargo");
    guard.apply_toolchain_env(&mut command);
    let values = command.get_envs().collect::<std::collections::HashMap<_, _>>();
    assert_eq!(values[OsStr::new("CARGO_HOME")], Some(real.join(".cargo").as_os_str()));
    assert_eq!(values[OsStr::new("RUSTUP_HOME")], Some(real.join(".rustup").as_os_str()));
    assert!(guard.path().join(".cargo/bin/cargo").is_file());
}

#[test]
fn remote_home_keeps_symlinked_operator_cargo_untouched() {
    let temp = tempfile::tempdir().expect("temp");
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let real = fixture_home(temp.path());
    let shared = temp.path().join("shared-cargo");
    fs::rename(real.join(".cargo"), &shared).expect("shared cache");
    symlink(&shared, real.join(".cargo")).expect("operator cargo symlink");
    let original = fs::read(shared.join("bin/cargo")).expect("original Cargo");
    let guard = IsolatedHomeGuard::create_from_home_with_remote_build(Some(&real), None, true).expect("home");
    assert!(!fs::symlink_metadata(guard.path().join(".cargo")).expect("private cargo").file_type().is_symlink());
    drop(guard);
    assert_eq!(fs::read_link(real.join(".cargo")).expect("operator symlink"), shared);
    assert_eq!(fs::read(shared.join("bin/cargo")).expect("operator Cargo"), original);
}

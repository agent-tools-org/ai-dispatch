// Executes the generated Cargo shim against fake rbox and host Cargo binaries.
// Covers routing, argument preservation, isolation, status diagnostics and heartbeats.
// Deps: tempfile, Bash and git; no real Cargo jobs or network calls.
use super::*;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

pub(super) fn executable(path: &Path, content: &str) {
    std::fs::write(path, content).expect("script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("executable");
}

struct Fixture { temp: TempDir, home: std::path::PathBuf, bin: std::path::PathBuf }
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        executable(&bin.join("rbox"), "#!/bin/bash\nprintf '%s\\n' \"$@\" > \"$RECORD\"\nprintf '%s' \"${CARGO_TARGET_DIR-unset}\" > \"$RECORD.target\"\nexit \"${STATUS:-0}\"\n");
        executable(&bin.join("cargo"), "#!/bin/bash\nprintf 'local\\n'; printf '%s\\n' \"$@\"\nexit \"${STATUS:-0}\"\n");
        configure(&mut Command::new("agent"), &home, "chosen-box").expect("install shim");
        let git = Command::new("git").args(["init", "-b", "feat/test.box"])
            .arg(temp.path()).output().expect("git init");
        assert!(git.status.success());
        let git = Command::new("git").current_dir(temp.path())
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "fixture"])
            .output().expect("git commit");
        assert!(git.status.success(), "{git:?}");
        Self { temp, home, bin }
    }

    fn command(&self, args: &[&str], status: u8) -> Command {
        let mut cmd = Command::new(self.home.join(".aid-build-shims/cargo"));
        cmd.env("PATH", format!("{}:{}:{}", self.home.join(".aid-build-shims").display(), self.bin.display(), std::env::var("PATH").expect("PATH")));
        cmd.env("RECORD", self.temp.path().join("argv")).env("STATUS", status.to_string());
        cmd.env_remove("AID_BUILD_JOBS");
        cmd.env("CARGO_TARGET_DIR", "/local/must-not-forward").env("AID_BUILD_BOX", "chosen-box");
        cmd.current_dir(self.temp.path()).args(args);
        cmd
    }
}

#[test]
fn forwards_build_set_original_args_untracked_and_remote_target() {
    let f = Fixture::new();
    for subcommand in ["build", "check", "test", "clippy", "bench", "doc"] {
        let output = f.command(&[subcommand, "--package", "space and 'quote'"], 0).output().expect("shim");
        assert!(output.status.success(), "{output:?}");
        let recorded = std::fs::read_to_string(f.temp.path().join("argv")).expect("argv");
        let args: Vec<_> = recorded.lines().collect();
        assert_eq!(&args[..2], ["exec", "chosen-box"]);
        assert!(args.contains(&"--untracked"));
        assert!(args.windows(2).any(|v| v == ["--jobs", "4"]));
        assert!(args.windows(2).any(|v| v == ["--timeout", "3600"]));
        assert!(args.windows(2).any(|v| v == ["--lock-timeout", "900"]));
        assert!(args.iter().any(|v| v.ends_with("/feat-test-box")));
        assert!(recorded.contains("export CARGO_TARGET_DIR=$HOME/.rbox/target/"));
        assert_eq!(&args[args.len()-4..], ["remote-cargo", subcommand, "--package", "space and 'quote'"]);
        assert_eq!(std::fs::read_to_string(f.temp.path().join("argv.target")).expect("target"), "unset");
    }
}

#[test]
fn local_passthrough_strips_shim_path_and_preserves_exit() {
    let f = Fixture::new();
    for subcommand in ["fmt", "metadata", "install", "update", "clean", "fetch", "run", "new", "help", "--version", "--list"] {
        let output = f.command(&[subcommand, "--verbose"], 17).output().expect("shim");
        assert_eq!(output.status.code(), Some(17));
        assert_eq!(String::from_utf8_lossy(&output.stdout), format!("local\n{subcommand}\n--verbose\n"));
        assert!(!f.temp.path().join("argv").exists());
    }
}

#[test]
fn lock_timeout_and_running_job_status_have_diagnostics() {
    let f = Fixture::new();
    for (status, message) in [(75, "box lock not acquired"), (124, "job still running on the box"), (23, "")] {
        let output = f.command(&["test"], status).output().expect("shim");
        assert_eq!(output.status.code(), Some(i32::from(status)));
        let stderr = String::from_utf8_lossy(&output.stderr);
        if message.is_empty() { assert!(stderr.is_empty(), "{stderr}"); }
        else { assert!(stderr.contains(message), "{stderr}"); }
    }
}

#[test]
fn toolchain_prefix_and_jobs_override_are_forwarded() {
    let f = Fixture::new();
    let output = f.command(&["+stable", "check"], 0).env("AID_BUILD_JOBS", "2").output().expect("shim");
    assert!(output.status.success());
    let args = std::fs::read_to_string(f.temp.path().join("argv")).expect("argv");
    assert!(args.contains("--jobs\n2\n"));
    assert!(args.ends_with("remote-cargo\n+stable\ncheck\n"));
}

#[test]
fn heartbeat_keeps_silent_remote_job_visible() {
    let f = Fixture::new();
    executable(&f.bin.join("sleep"), "#!/bin/bash\nexec /bin/sleep 0.05\n");
    executable(&f.bin.join("rbox"), "#!/bin/bash\n/bin/sleep 0.2\nexit 0\n");
    let output = f.command(&["check"], 0).output().expect("shim");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("[remote-build] chosen-box: still running (60s)"));
}

#[test]
fn global_options_do_not_bypass_remote_routing() {
    let f = Fixture::new();
    for args in [vec!["--color", "always", "check"], vec!["--config", "net.offline=true", "build"], vec!["--config=net.offline=true", "test"]] {
        let output = f.command(&args, 0).output().expect("shim");
        assert!(output.status.success(), "{output:?}");
        let recorded = std::fs::read_to_string(f.temp.path().join("argv")).expect("argv");
        assert!(recorded.ends_with(&format!("remote-cargo\n{}\n", args.join("\n"))));
    }
}

#[test]
fn local_passthrough_preserves_empty_path_entries() {
    let f = Fixture::new();
    executable(&f.temp.path().join("cargo"), "#!/bin/bash\necho cwd-cargo\n");
    let utilities = f.temp.path().join("utilities");
    std::fs::create_dir(&utilities).expect("utilities");
    for name in ["bash", "dirname"] {
        std::os::unix::fs::symlink(format!("/bin/{name}"), utilities.join(name)).expect("utility");
    }
    let output = f.command(&["fmt"], 0)
        .env("PATH", format!("{}:{}:", f.home.join(".aid-build-shims").display(), utilities.display()))
        .output().expect("shim");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "cwd-cargo\n");
}

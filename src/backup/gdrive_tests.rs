// Tests for the gws-backed Google Drive target using a fake `gws` script.
// The script records argv and cwd; gws itself is never invoked.

use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

struct Fake {
    _dir: tempfile::TempDir,
    binary: PathBuf,
    log: PathBuf,
}

/// `list_reply` is what the fake prints for `files list`; everything else
/// answers `{"id":"fake123"}`.
fn fake_gws(list_reply: &str) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("gws.log");
    let binary = dir.path().join("gws");
    let script = format!(
        "#!/bin/sh\nprintf 'cwd=%s argv=%s\\n' \"$(pwd)\" \"$*\" >> '{}'\n\
         case \"$*\" in *'files list'*) echo '{list_reply}';; *) echo '{{\"id\":\"fake123\"}}';; esac\n",
        log.display()
    );
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    Fake { _dir: dir, binary, log }
}

fn bundle_in(dir: &Path) -> PathBuf {
    let path = dir.join("t-1-abc1234.tar.gz");
    fs::write(&path, b"bundle").unwrap();
    path
}

#[test]
fn upload_creates_missing_folders_then_uploads_from_bundle_dir() {
    let fake = fake_gws(r#"{"files":[]}"#);
    let bundle_dir = tempfile::tempdir().unwrap();
    let bundle = bundle_in(bundle_dir.path());
    let target = GdriveTarget::with_binary(&fake.binary);
    let dest = BackupDest { folder: "aid-backups/proj".into(), file_name: "t-1-abc1234.tar.gz".into() };

    let backup = target.upload(&bundle, &dest).unwrap();

    assert_eq!(backup.id, "fake123");
    assert_eq!(backup.url, "https://drive.google.com/file/d/fake123/view");
    let log = fs::read_to_string(&fake.log).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 5, "{log}");
    assert!(lines[0].contains("drive files list --params") && lines[0].contains("'root' in parents"));
    assert!(lines[1].contains("drive files create --json") && lines[1].contains(FOLDER_MIME));
    assert!(lines[2].contains("'fake123' in parents") && lines[2].contains("name = 'proj'"));
    assert!(lines[4].contains("--upload t-1-abc1234.tar.gz"), "{}", lines[4]);
    assert!(lines[4].contains(r#""parents":["fake123"]"#));
    let expected_cwd = bundle_dir.path().canonicalize().unwrap();
    assert!(lines[4].starts_with(&format!("cwd={}", expected_cwd.display())), "{}", lines[4]);
}

#[test]
fn upload_reuses_existing_folder_without_creating() {
    let fake = fake_gws(r#"{"files":[{"id":"existing","name":"audits"}]}"#);
    let bundle_dir = tempfile::tempdir().unwrap();
    let bundle = bundle_in(bundle_dir.path());
    let dest = BackupDest { folder: "audits".into(), file_name: "t-1-abc1234.tar.gz".into() };

    GdriveTarget::with_binary(&fake.binary).upload(&bundle, &dest).unwrap();

    let log = fs::read_to_string(&fake.log).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    let created_folder = log.lines().any(|l| l.contains("files create") && l.contains(FOLDER_MIME));
    assert!(!created_folder, "{log}");
    assert!(log.contains(r#""parents":["existing"]"#));
}

#[test]
fn missing_gws_names_install_and_login_steps() {
    let bundle_dir = tempfile::tempdir().unwrap();
    let bundle = bundle_in(bundle_dir.path());
    let dest = BackupDest { folder: String::new(), file_name: "x".into() };
    let target = GdriveTarget::with_binary(bundle_dir.path().join("no-such-gws"));

    let err = target.upload(&bundle, &dest).unwrap_err().to_string();

    assert!(err.contains("npm install -g @googleworkspace/cli"), "{err}");
    assert!(err.contains("gws auth login"), "{err}");
}

#[test]
fn gws_failure_surfaces_stderr_and_login_hint() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("gws");
    fs::write(&binary, "#!/bin/sh\necho 'not authenticated' >&2\nexit 3\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let bundle = bundle_in(dir.path());
    let dest = BackupDest { folder: "a".into(), file_name: "x".into() };

    let err = GdriveTarget::with_binary(&binary).upload(&bundle, &dest).unwrap_err().to_string();

    assert!(err.contains("not authenticated"), "{err}");
    assert!(err.contains("gws auth login"), "{err}");
}

#[test]
fn folder_names_are_escaped_in_queries() {
    assert_eq!(escape_query("it's \\ here"), "it\\'s \\\\ here");
}

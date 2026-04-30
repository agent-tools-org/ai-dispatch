// Handler for `aid byok` — wraps the embedded BYOK shell scripts so users
// installed from crates.io get the same apply/remove/probe flow.
// Exports: BYOK_LIB, BYOK_APPLY, BYOK_REMOVE, BYOK_PROBE, BYOK_EXAMPLE_MIMO, BYOK_DOC,
//   ByokAction, run_byok_command. Deps: std::process, std::fs, anyhow.

use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const BYOK_LIB: &str = include_str!("../../scripts/aid-byok-lib.sh");
pub const BYOK_APPLY: &str = include_str!("../../scripts/aid-byok-apply.sh");
pub const BYOK_REMOVE: &str = include_str!("../../scripts/aid-byok-remove.sh");
pub const BYOK_PROBE: &str = include_str!("../../scripts/aid-byok-probe.sh");
pub const BYOK_EXAMPLE_MIMO: &str = include_str!("../../examples/byok/mimo.toml");
pub const BYOK_DOC: &str = include_str!("../../docs/byok-pattern.md");

pub enum ByokAction {
    Apply {
        manifest: PathBuf,
        dry_run: bool,
        key: Option<String>,
    },
    Remove {
        target: String,
    },
    Probe {
        manifest: PathBuf,
        key: Option<String>,
    },
    Example,
    Doc,
}

pub fn run_byok_command(action: ByokAction) -> Result<()> {
    match action {
        ByokAction::Apply { manifest, dry_run, key } => apply(manifest, dry_run, key),
        ByokAction::Remove { target } => remove(target),
        ByokAction::Probe { manifest, key } => probe(manifest, key),
        ByokAction::Example => {
            print!("{}", BYOK_EXAMPLE_MIMO);
            Ok(())
        }
        ByokAction::Doc => {
            print!("{}", BYOK_DOC);
            Ok(())
        }
    }
}

fn apply(manifest: PathBuf, dry_run: bool, key: Option<String>) -> Result<()> {
    let mut args: Vec<String> = Vec::new();
    if dry_run {
        args.push("--dry-run".into());
    }
    if let Some(k) = key {
        args.push("--key".into());
        args.push(k);
    }
    args.push(absolutize(&manifest)?);
    run_script(Script::Apply, &args)
}

fn remove(target: String) -> Result<()> {
    // Pass through file paths verbatim; if it's a path, absolutize so the script
    // resolves it after we change directory under a temp script dir is unaffected
    // (the script does not chdir, but absolute is harmless and avoids surprises).
    let arg = if Path::new(&target).is_file() {
        absolutize(Path::new(&target))?
    } else {
        target
    };
    run_script(Script::Remove, &[arg])
}

fn probe(manifest: PathBuf, key: Option<String>) -> Result<()> {
    let mut args: Vec<String> = Vec::new();
    if let Some(k) = key {
        args.push("--key".into());
        args.push(k);
    }
    args.push(absolutize(&manifest)?);
    run_script(Script::Probe, &args)
}

#[derive(Clone, Copy)]
enum Script {
    Apply,
    Remove,
    Probe,
}

impl Script {
    fn filename(self) -> &'static str {
        match self {
            Script::Apply => "aid-byok-apply.sh",
            Script::Remove => "aid-byok-remove.sh",
            Script::Probe => "aid-byok-probe.sh",
        }
    }
}

fn run_script<S: AsRef<OsStr>>(script: Script, args: &[S]) -> Result<()> {
    let _guard = ScriptDir::extract().context("preparing embedded BYOK scripts")?;
    let script_path = _guard.path.join(script.filename());
    let status = Command::new("bash")
        .arg(&script_path)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn bash for {}", script.filename()))?;
    if !status.success() {
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }
    Ok(())
}

struct ScriptDir {
    path: PathBuf,
}

impl ScriptDir {
    fn extract() -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("aid-byok-{}-{}", pid, nanos));
        fs::create_dir_all(&path)
            .with_context(|| format!("creating temp script dir {}", path.display()))?;
        write_script(&path, "aid-byok-lib.sh", BYOK_LIB, 0o644)?;
        write_script(&path, "aid-byok-apply.sh", BYOK_APPLY, 0o755)?;
        write_script(&path, "aid-byok-remove.sh", BYOK_REMOVE, 0o755)?;
        write_script(&path, "aid-byok-probe.sh", BYOK_PROBE, 0o755)?;
        Ok(Self { path })
    }
}

impl Drop for ScriptDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_script(dir: &Path, name: &str, body: &str, mode: u32) -> Result<()> {
    let path = dir.join(name);
    fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {}", path.display()))?;
    Ok(())
}

fn absolutize(path: &Path) -> Result<String> {
    if !path.exists() {
        bail!("manifest not found: {}", path.display());
    }
    let abs = fs::canonicalize(path)
        .with_context(|| format!("canonicalizing manifest path {}", path.display()))?;
    Ok(abs.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scripts_have_expected_markers() {
        assert!(BYOK_LIB.contains("aid-byok-generated"));
        assert!(BYOK_APPLY.contains("Usage: scripts/aid-byok-apply.sh"));
        assert!(BYOK_REMOVE.contains("Usage: scripts/aid-byok-remove.sh"));
        assert!(BYOK_PROBE.contains("tool_calls: yes"));
    }

    #[test]
    fn example_manifest_is_valid_toml() {
        let parsed: toml::Value =
            toml::from_str(BYOK_EXAMPLE_MIMO).expect("example manifest must parse");
        let id = parsed
            .get("byok")
            .and_then(|b| b.get("id"))
            .and_then(|v| v.as_str())
            .expect("manifest.byok.id must be a string");
        assert!(!id.is_empty());
    }

    #[test]
    fn extracts_scripts_to_temp_dir() {
        let guard = ScriptDir::extract().expect("extract");
        for name in [
            "aid-byok-lib.sh",
            "aid-byok-apply.sh",
            "aid-byok-remove.sh",
            "aid-byok-probe.sh",
        ] {
            let p = guard.path.join(name);
            assert!(p.is_file(), "{} should exist", p.display());
        }
        let mode = fs::metadata(guard.path.join("aid-byok-apply.sh"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755);
        let dir = guard.path.clone();
        drop(guard);
        assert!(!dir.exists(), "temp dir should be cleaned up");
    }

    #[test]
    fn absolutize_rejects_missing() {
        let err = absolutize(Path::new("/definitely/does/not/exist/byok.toml"));
        assert!(err.is_err());
    }
}

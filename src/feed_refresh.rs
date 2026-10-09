// Shared detached downloader and atomic adoption for model feed caches.
// Exports: maybe_refresh; callers supply feed validation and freshness checks.
// Deps: filesystem, shell/curl, libc setsid on Unix.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const PART_TTL: Duration = Duration::from_secs(60);
const DOWNLOAD: &str = r#"curl -sfL --max-time 15 -o "$1.part" "$2" && mv "$1.part" "$1.ready""#;

pub(crate) fn maybe_refresh(
    path: &Path,
    url: &str,
    valid: impl FnOnce(&[u8]) -> bool,
    fresh: impl FnOnce() -> bool,
) {
    adopt_ready(path, valid);
    if fresh() {
        return;
    }
    let part = sibling(path, ".part");
    if !reserve_part(&part) {
        return;
    }
    let mut child = Command::new("sh");
    child
        .args(["-c", DOWNLOAD, "aid-feed-refresh"])
        .arg(path)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only the async-signal-safe syscall runs between fork and exec.
        unsafe {
            child.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    if child.spawn().is_err() {
        let _ = fs::remove_file(part);
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn adopt_ready(path: &Path, valid: impl FnOnce(&[u8]) -> bool) {
    let ready = sibling(path, ".ready");
    let Ok(bytes) = fs::read(&ready) else { return };
    if valid(&bytes) {
        let _ = fs::rename(&ready, path);
    } else {
        let _ = fs::remove_file(ready);
    }
}

fn reserve_part(part: &Path) -> bool {
    if let Ok(metadata) = fs::metadata(part) {
        let age = metadata
            .modified()
            .ok()
            .and_then(|time| time.elapsed().ok());
        if age.is_none_or(|age| age < PART_TTL) {
            return false;
        }
        if fs::remove_file(part).is_err() {
            return false;
        }
    }
    let Some(parent) = part.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(part)
        .is_ok()
}

#[cfg(test)]
#[path = "feed_refresh_tests.rs"]
mod tests;

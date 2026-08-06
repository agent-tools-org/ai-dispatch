// Isolated HOME directory per task dispatch to prevent orchestrator identity leaks.
// Exports: IsolatedHomeGuard, DEFAULT_DENYLIST.
// Deps: std::fs, std::path::{Path, PathBuf}.

use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_DENYLIST: &[&str] = &[
    ".claude",
    ".claude.json",
    ".anthropic",
];

pub struct IsolatedHomeGuard {
    path: PathBuf,
}

impl IsolatedHomeGuard {
    pub fn create(task_id: Option<&str>) -> anyhow::Result<Self> {
        let real_home = std::env::var_os("HOME").map(PathBuf::from);
        Self::create_from_home(real_home.as_deref(), task_id)
    }

    pub fn create_from_home(real_home: Option<&Path>, task_id: Option<&str>) -> anyhow::Result<Self> {
        let base_dir = match task_id {
            Some(id) => crate::paths::task_dir(id),
            None => {
                let pid = std::process::id();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                crate::paths::aid_dir()
                    .join("tmp_home")
                    .join(format!("iso-{pid}-{now}"))
            }
        };
        let target_home = base_dir.join("home");
        Self::build_isolated_home(real_home, &target_home)?;
        Ok(Self { path: target_home })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn build_isolated_home(real_home: Option<&Path>, isolated_path: &Path) -> anyhow::Result<()> {
        if isolated_path.exists() {
            let _ = fs::remove_dir_all(isolated_path);
        }
        fs::create_dir_all(isolated_path)?;

        let Some(real_home) = real_home else {
            return Ok(());
        };
        if !real_home.is_dir() {
            return Ok(());
        }

        let entries = match fs::read_dir(real_home) {
            Ok(rd) => rd,
            Err(_) => return Ok(()),
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            if DEFAULT_DENYLIST.contains(&name_str.as_ref()) {
                continue;
            }
            let link_dest = isolated_path.join(&file_name);
            let target_path = entry.path();
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&target_path, &link_dest);
            }
        }
        Ok(())
    }
}

impl Drop for IsolatedHomeGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
#[path = "home_isolation_tests.rs"]
mod tests;

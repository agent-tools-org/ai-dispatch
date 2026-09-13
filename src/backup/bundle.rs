// Materializes a task's artifacts into a temp dir and packs them with the
// system `tar` into `<task_id>-<short_sha>.tar.gz`. Exports: Bundle, build.
// Deps: cmd::export markdown, cmd::show worktree diff, paths::log_path.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::config::Artifact;
use crate::store::Store;
use crate::types::Task;

/// A packed bundle. The temp directory holding it is removed on drop, so the
/// target must finish uploading before the bundle goes out of scope.
pub(crate) struct Bundle {
    pub dir: PathBuf,
    pub path: PathBuf,
    pub file_name: String,
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub(crate) fn build(store: &Store, task: &Task, include: &[Artifact]) -> Result<Bundle> {
    let dir = std::env::temp_dir().join(format!("aid-backup-{}-{}", task.id, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let stage = dir.join("stage");
    fs::create_dir_all(&stage).with_context(|| format!("creating {}", stage.display()))?;
    let file_name = format!("{}-{}.tar.gz", task.id, short_sha(task));
    let bundle = Bundle { path: dir.join(&file_name), dir, file_name };
    let staged = stage_artifacts(store, task, include, &stage)?;
    if staged == 0 {
        bail!("no artifacts to back up (include = {include:?})");
    }
    pack(&stage, &bundle.path)?;
    Ok(bundle)
}

fn stage_artifacts(store: &Store, task: &Task, include: &[Artifact], stage: &Path) -> Result<usize> {
    let mut staged = 0;
    for artifact in include {
        match artifact {
            Artifact::Export => {
                let markdown = crate::cmd::export::render_markdown(store, task.id.as_str())?;
                fs::write(stage.join("export.md"), markdown)?;
            }
            Artifact::Diff => {
                let diff = crate::cmd::show::worktree_diff(task, task.id.as_str())?;
                fs::write(stage.join("diff.patch"), diff)?;
            }
            Artifact::Transcript => {
                let log = crate::paths::log_path(task.id.as_str());
                if !log.is_file() {
                    continue;
                }
                fs::copy(&log, stage.join("transcript.jsonl"))
                    .with_context(|| format!("copying {}", log.display()))?;
            }
        }
        staged += 1;
    }
    Ok(staged)
}

fn pack(stage: &Path, archive: &Path) -> Result<()> {
    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(stage)
        .arg(".")
        .output()
        .context("running tar")?;
    if !output.status.success() {
        bail!("tar failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

pub(crate) fn short_sha(task: &Task) -> String {
    if let Some(sha) = task.final_head_sha.as_deref().filter(|s| s.len() >= 7) {
        return sha[..7].to_string();
    }
    task.worktree_path
        .as_deref()
        .filter(|path| Path::new(path).exists())
        .and_then(|path| {
            Command::new("git")
                .args(["-C", path, "rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| "nosha".to_string())
}

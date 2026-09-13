// Backup settings: project `[backup]`, global `[backup.gdrive]`, CLI override,
// and folder template expansion. Exports config types, resolve, expand_folder.
// Deps: serde, RunArgs (persisted dispatch args), project/global config loaders.

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{Task, TaskStatus};

const DEFAULT_FOLDER: &str = "aid-backups/{project}";

/// `[backup]` in `.aid/project.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupProjectConfig {
    pub target: Option<String>,
    pub folder: Option<String>,
    pub include: Option<Vec<String>>,
    pub on: Option<Vec<String>>,
}

/// `[backup]` in `~/.aid/config.toml`: per-target global defaults.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BackupGlobalConfig {
    pub gdrive: GdriveGlobalConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GdriveGlobalConfig {
    pub folder: Option<String>,
    /// Explicit `gws` binary; defaults to `gws` on PATH.
    pub binary: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Artifact {
    Export,
    Diff,
    Transcript,
}

impl Artifact {
    pub(crate) const ALL: [Self; 3] = [Self::Export, Self::Diff, Self::Transcript];

    fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "export" => Ok(Self::Export),
            "diff" => Ok(Self::Diff),
            "transcript" => Ok(Self::Transcript),
            other => bail!("unknown backup include '{other}' (expected export, diff, transcript)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Complete,
    Fail,
}

impl Trigger {
    fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "complete" => Ok(Self::Complete),
            "fail" => Ok(Self::Fail),
            other => bail!("unknown backup trigger '{other}' (expected complete, fail)"),
        }
    }

    pub(crate) fn for_status(status: TaskStatus) -> Option<Self> {
        match status {
            TaskStatus::Done => Some(Self::Complete),
            TaskStatus::Failed => Some(Self::Fail),
            _ => None,
        }
    }
}

/// Fully resolved backup settings for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupSettings {
    pub target: String,
    pub folder: String,
    pub include: Vec<Artifact>,
    pub on: Vec<Trigger>,
    /// Target binary override from the global config, if any.
    pub binary: Option<PathBuf>,
}

/// Parses `--backup TARGET[:FOLDER]`.
pub(crate) fn parse_cli_spec(spec: &str) -> Result<(String, Option<String>)> {
    let (target, folder) = match spec.split_once(':') {
        Some((target, folder)) => (target, Some(folder.trim_matches('/').to_string())),
        None => (spec, None),
    };
    let target = target.trim();
    if target.is_empty() {
        bail!("--backup needs a target name, e.g. gdrive or gdrive:folder");
    }
    Ok((target.to_string(), folder.filter(|f| !f.is_empty())))
}

/// CLI wins over project; the global config only supplies a default folder.
/// Returns `None` when no backup is configured or `--no-backup` was given.
pub(crate) fn resolve(
    cli: Option<&str>,
    no_backup: bool,
    project: Option<&BackupProjectConfig>,
    global: &BackupGlobalConfig,
) -> Result<Option<BackupSettings>> {
    if no_backup {
        return Ok(None);
    }
    let (cli_target, cli_folder) = match cli {
        Some(spec) => {
            let (target, folder) = parse_cli_spec(spec)?;
            (Some(target), folder)
        }
        None => (None, None),
    };
    let Some(target) = cli_target.or_else(|| project.and_then(|p| p.target.clone())) else {
        return Ok(None);
    };
    let (global_folder, binary) = match target.as_str() {
        "gdrive" => (global.gdrive.folder.clone(), global.gdrive.binary.clone()),
        _ => (None, None),
    };
    let folder = cli_folder
        .or_else(|| project.and_then(|p| p.folder.clone()))
        .or(global_folder)
        .unwrap_or_else(|| DEFAULT_FOLDER.to_string());
    let include = match project.and_then(|p| p.include.as_ref()) {
        Some(values) => values.iter().map(|v| Artifact::parse(v)).collect::<Result<_>>()?,
        None => Artifact::ALL.to_vec(),
    };
    let on = match project.and_then(|p| p.on.as_ref()) {
        Some(values) => values.iter().map(|v| Trigger::parse(v)).collect::<Result<_>>()?,
        None => vec![Trigger::Complete, Trigger::Fail],
    };
    Ok(Some(BackupSettings { target, folder, include, on, binary }))
}

/// Resolves settings for a stored task from its persisted dispatch args, the
/// project config next to its repo, and the global config. A task without
/// persisted args never reached dispatch (setup failed before they were saved),
/// so its `--backup` / `--no-backup` intent is unknown and it is not backed up.
pub(crate) fn resolve_for_task(store: &Store, task_id: &str) -> Result<Option<BackupSettings>> {
    let Some(args) = RunArgs::saved_for_task(store, task_id)? else {
        return Ok(None);
    };
    let (cli, no_backup) = (args.backup.as_deref(), args.no_backup);
    if no_backup {
        return Ok(None);
    }
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("task '{task_id}' not found"))?;
    let project = project_backup(&task);
    if cli.is_none() && project.as_ref().and_then(|p| p.target.as_ref()).is_none() {
        return Ok(None);
    }
    let global = crate::config::load_config()?.backup;
    resolve(cli, no_backup, project.as_ref(), &global)
}

fn project_backup(task: &Task) -> Option<BackupProjectConfig> {
    let dir = task.repo_path.as_deref().or(task.worktree_path.as_deref())?;
    crate::project::detect_project_in(Path::new(dir))?.backup
}

/// Expands `{project}`, `{date}`, `{task_id}` and `{branch}`.
pub(crate) fn expand_folder(template: &str, task: &Task) -> String {
    let project = task.project_id.as_deref().unwrap_or("unknown");
    let branch = task
        .worktree_branch
        .as_deref()
        .or(task.final_branch.as_deref())
        .unwrap_or("no-branch");
    template
        .replace("{project}", project)
        .replace("{date}", &chrono::Local::now().format("%Y-%m-%d").to_string())
        .replace("{task_id}", task.id.as_str())
        .replace("{branch}", branch)
        .trim_matches('/')
        .to_string()
}

// Google Drive backup target driven by the `gws` CLI (@googleworkspace/cli).
// Exports: GdriveTarget. Resolves/creates the folder path, then uploads the
// bundle from its own directory because gws only uploads paths inside its cwd.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{BackupDest, BackupRef, BackupTarget};

const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const INSTALL_HINT: &str =
    "install it with `npm install -g @googleworkspace/cli`, then run `gws auth login`";

pub struct GdriveTarget {
    binary: PathBuf,
}

impl GdriveTarget {
    pub fn new() -> Self {
        Self { binary: PathBuf::from("gws") }
    }

    /// Use an explicit `gws` binary (tests point this at a fake script).
    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self { binary: binary.into() }
    }

    fn run(&self, cwd: Option<&Path>, args: &[&str]) -> Result<Value> {
        let mut command = Command::new(&self.binary);
        command.args(args);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let output = match command.output() {
            Ok(output) => output,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                bail!("`gws` not found on PATH; {INSTALL_HINT}")
            }
            Err(err) => return Err(err).context("spawning gws"),
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "gws {} exited with {}: {}. If gws is not signed in, run `gws auth login`",
                args.get(2).copied().unwrap_or("?"),
                output.status,
                stderr.trim(),
            );
        }
        serde_json::from_slice(&output.stdout).context("parsing gws JSON output")
    }

    fn resolve_folder(&self, folder: &str) -> Result<String> {
        let mut parent = "root".to_string();
        for segment in folder.split('/').filter(|s| !s.is_empty()) {
            parent = match self.find_folder(&parent, segment)? {
                Some(id) => id,
                None => self.create_folder(&parent, segment)?,
            };
        }
        Ok(parent)
    }

    fn find_folder(&self, parent: &str, name: &str) -> Result<Option<String>> {
        let query = format!(
            "name = '{}' and mimeType = '{FOLDER_MIME}' and '{parent}' in parents and trashed = false",
            escape_query(name)
        );
        let params = json!({"q": query, "fields": "files(id,name)", "pageSize": 1}).to_string();
        let value = self.run(None, &["drive", "files", "list", "--params", &params])?;
        let files = match &value {
            Value::Array(files) => files.as_slice(),
            other => other.get("files").and_then(Value::as_array).map_or(&[][..], Vec::as_slice),
        };
        Ok(files.first().and_then(|f| f.get("id")).and_then(Value::as_str).map(str::to_string))
    }

    fn create_folder(&self, parent: &str, name: &str) -> Result<String> {
        let body = json!({"name": name, "mimeType": FOLDER_MIME, "parents": [parent]}).to_string();
        let value = self.run(None, &["drive", "files", "create", "--json", &body])?;
        file_id(&value).with_context(|| format!("creating Drive folder '{name}'"))
    }
}

impl Default for GdriveTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl BackupTarget for GdriveTarget {
    fn name(&self) -> &str {
        "gdrive"
    }

    fn upload(&self, bundle: &Path, dest: &BackupDest) -> Result<BackupRef> {
        let cwd = bundle.parent().ok_or_else(|| anyhow!("bundle path has no parent"))?;
        let file_name = bundle
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("bundle path has no file name"))?;
        let parent = self.resolve_folder(&dest.folder)?;
        let body = json!({"name": dest.file_name, "parents": [parent]}).to_string();
        let value = self.run(
            Some(cwd),
            &["drive", "files", "create", "--json", &body, "--upload", file_name],
        )?;
        let id = file_id(&value).context("uploading bundle")?;
        let url = format!("https://drive.google.com/file/d/{id}/view");
        Ok(BackupRef { id, url })
    }
}

fn file_id(value: &Value) -> Result<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("gws response has no file id: {value}"))
}

fn escape_query(name: &str) -> String {
    name.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
#[path = "gdrive_tests.rs"]
mod gdrive_tests;

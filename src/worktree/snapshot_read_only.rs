// Content and index snapshots for read-only settlement, including nested Git HEADs.
// Exports capture to the worktree snapshot reader; hashes regular files in one batch.
// Deps: git CLI, filesystem metadata, serde_json, and snapshot path ownership.

use anyhow::{Context, Result};
use std::{io::Write, os::unix::fs::PermissionsExt, path::Path, process::{Command, Stdio}};

pub(super) fn capture(dir: &Path) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    for tracked in [true, false] {
        let args = if tracked { ["ls-files", "-z", "--stage", "--cached"] }
            else { ["ls-files", "-z", "--others", "--exclude-standard"] };
        let listed = Command::new("git").current_dir(dir).args(args).output()?;
        anyhow::ensure!(listed.status.success(), "Read-only snapshot path listing failed: {}", String::from_utf8_lossy(&listed.stderr));
        for entry in listed.stdout.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
            let entry = std::str::from_utf8(entry).context("Non-UTF-8 snapshot path")?;
            let (index, path) = if tracked { entry.split_once('\t').context("Missing index entry")? }
                else { ("", entry) };
            let path = path.trim_end_matches('/');
            if super::is_aid_owned_path(path) { continue; }
            let (content, mode) = file_state(dir, path, index)?;
            entries.push((path.to_string(), index.to_string(), content, mode));
        }
    }
    let paths: Vec<_> = entries.iter().filter(|entry| entry.2.is_empty()).map(|entry| entry.0.as_str()).collect();
    let hashes = hash_files(dir, &paths)?;
    let mut hashes = hashes.into_iter();
    let mut state = Vec::new();
    for (path, index, content, mode) in entries {
        let content = if content.is_empty() { hashes.next().context("Missing file hash")? } else { content };
        state.push(serde_json::to_string(&(path, index, content, mode))?);
    }
    state.sort();
    Ok(state)
}

fn file_state(dir: &Path, path: &str, index: &str) -> Result<(String, u32)> {
    let file = dir.join(path);
    match std::fs::symlink_metadata(&file) {
        Ok(meta) if meta.is_symlink() => Ok((format!("symlink:{}", std::fs::read_link(&file)?.display()), 0)),
        Ok(meta) if meta.is_file() => Ok((String::new(), meta.permissions().mode())),
        Ok(meta) if meta.is_dir() => {
            if index.starts_with("160000 ") && !file.join(".git").exists() {
                return Ok((format!("gitlink:{}", index.split_whitespace().nth(1).context("Missing gitlink commit")?), 0));
            }
            anyhow::ensure!(file.join(".git").exists(), "Unsupported read-only snapshot directory: {path}");
            let head = Command::new("git").current_dir(&file).args(["rev-parse", "--verify", "HEAD"]).output()?;
            anyhow::ensure!(head.status.success(), "Failed to snapshot repository {path}: {}", String::from_utf8_lossy(&head.stderr));
            Ok((format!("gitlink:{}", String::from_utf8(head.stdout)?.trim()), 0))
        }
        Ok(_) => anyhow::bail!("Unsupported read-only snapshot path: {path}"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(("missing".into(), 0)),
        Err(err) => Err(err).with_context(|| format!("Failed to snapshot {path}")),
    }
}

fn hash_files(dir: &Path, paths: &[&str]) -> Result<Vec<String>> {
    if paths.is_empty() { return Ok(Vec::new()); }
    let mut input = Vec::new();
    let dir = dir.canonicalize()?;
    for path in paths {
        let file = dir.join(path);
        let path = file.to_str().context("Non-UTF-8 snapshot path")?;
        // Git accepts C-quoted paths, so tabs and newlines cannot split records.
        input.push(b'"');
        for byte in path.bytes() {
            match byte {
                b'"' | b'\\' => { input.push(b'\\'); input.push(byte); }
                32..=126 => input.push(byte),
                _ => write!(input, "\\{byte:03o}")?,
            }
        }
        input.extend_from_slice(b"\"\n");
    }
    let mut child = Command::new("git").current_dir(&dir)
        .args(["hash-object", "--no-filters", "--stdin-paths"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().context("Missing hash-object stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output()?;
    writer.join().map_err(|_| anyhow::anyhow!("Snapshot writer panicked"))??;
    anyhow::ensure!(output.status.success(), "Read-only file hashing failed: {}", String::from_utf8_lossy(&output.stderr));
    let hashes: Vec<_> = String::from_utf8(output.stdout)?.lines().map(str::to_string).collect();
    anyhow::ensure!(hashes.len() == paths.len(), "Read-only file hash count mismatch");
    Ok(hashes)
}

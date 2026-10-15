// Content and index snapshots for read-only settlement, including nested Git HEADs.
// Exports capture to the worktree snapshot reader; hashes regular files in one batch.
// Deps: git CLI, filesystem metadata, serde_json, and snapshot path ownership.

use anyhow::{Context, Result};
use std::{io::Write, os::unix::fs::{MetadataExt, PermissionsExt}, path::Path, process::{Command, Stdio}};

pub(super) fn capture(dir: &Path) -> Result<Vec<String>> {
    let root = Command::new("git").current_dir(dir).args(["rev-parse", "--show-toplevel"]).output()?;
    anyhow::ensure!(root.status.success(), "Failed to resolve read-only repository root");
    let root = String::from_utf8(root.stdout)?;
    let dir = Path::new(root.trim_end_matches('\n'));
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
    let mut state = git_state(dir)?;
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
            if file.join(".git").exists() {
                Ok((format!("gitlink:{}", head_state(&file)?), 0))
            } else {
                Ok(("directory".into(), meta.permissions().mode()))
            }
        }
        Ok(meta) => Ok((format!("special:{}:{}:{}:{}:{}", meta.mode(), meta.rdev(),
            meta.ino(), meta.mtime(), meta.mtime_nsec()), meta.permissions().mode())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(("missing".into(), 0)),
        Err(err) => Err(err).with_context(|| format!("Failed to snapshot {path}")),
    }
}

fn head_state(dir: &Path) -> Result<String> {
    let symbolic = Command::new("git").current_dir(dir).args(["symbolic-ref", "-q", "HEAD"]).output()?;
    anyhow::ensure!(symbolic.status.success() || symbolic.status.code() == Some(1),
        "Failed to snapshot symbolic HEAD: {}", String::from_utf8_lossy(&symbolic.stderr));
    let head = Command::new("git").current_dir(dir)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"]).output()?;
    anyhow::ensure!(head.status.success() || head.status.code() == Some(1),
        "Failed to snapshot HEAD: {}", String::from_utf8_lossy(&head.stderr));
    let oid = if head.status.success() { String::from_utf8(head.stdout)? } else { "unborn".into() };
    Ok(format!("{}:{}", String::from_utf8(symbolic.stdout)?.trim(), oid.trim()))
}

fn git_state(dir: &Path) -> Result<Vec<String>> {
    let mut state = vec![serde_json::to_string(&(".git/HEAD", "", head_state(dir)?, 0u32))?];
    let refs = Command::new("git").current_dir(dir)
        .args(["for-each-ref", "--format=%(refname) %(objectname) %(symref)"]).output()?;
    anyhow::ensure!(refs.status.success(), "Failed to snapshot refs: {}",
        String::from_utf8_lossy(&refs.stderr));
    for entry in String::from_utf8(refs.stdout)?.lines() {
        let (name, content) = entry.split_once(' ').context("Malformed read-only ref")?;
        state.push(serde_json::to_string(&(format!(".git/{name}"), "", content, 0u32))?);
    }
    Ok(state)
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

#[cfg(test)]
#[path = "snapshot_read_only_tests.rs"]
mod tests;

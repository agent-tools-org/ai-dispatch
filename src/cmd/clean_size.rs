// Directory size measurement for cleanup reporting and session hints.
// Exports: exact and bounded recursive size scanners, hint limits.
// Deps: anyhow, std::fs, std::path.

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[cfg(not(unix))]
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub(crate) const CLEANUP_HINT_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;
pub(crate) const CLEANUP_HINT_ENTRY_LIMIT: usize = 100_000;
pub(crate) const CLEANUP_HINT_BYTE_LIMIT: u64 = CLEANUP_HINT_THRESHOLD_BYTES * 10;

#[derive(Default)]
pub(crate) struct SizeTracker {
    seen: HashSet<FileIdentity>,
}

impl SizeTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get_dir_size(&mut self, path: &Path) -> Result<u64> {
        let mut total = 0;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                total += self.get_dir_size(&entry.path())?;
            } else if file_type.is_file() {
                total += self.file_size(&entry.path(), &entry.metadata()?);
            }
        }
        Ok(total)
    }

    pub(crate) fn get_dir_size_bounded(
        &mut self,
        path: &Path,
        byte_limit: u64,
        entry_limit: usize,
    ) -> Result<(u64, usize)> {
        let mut bytes = 0;
        let mut entries = 0;
        self.measure_dir_size_bounded(path, byte_limit, entry_limit, &mut bytes, &mut entries)?;
        Ok((bytes, entries))
    }

    fn measure_dir_size_bounded(
        &mut self,
        path: &Path,
        byte_limit: u64,
        entry_limit: usize,
        bytes: &mut u64,
        entries: &mut usize,
    ) -> Result<()> {
        if *bytes >= byte_limit || *entries >= entry_limit {
            return Ok(());
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            *entries += 1;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                self.measure_dir_size_bounded(
                    &entry.path(), byte_limit, entry_limit, bytes, entries,
                )?;
            } else if file_type.is_file() {
                *bytes += self.file_size(&entry.path(), &entry.metadata()?);
            }
            if *bytes >= byte_limit || *entries >= entry_limit {
                break;
            }
        }
        Ok(())
    }

    fn file_size(&mut self, path: &Path, metadata: &fs::Metadata) -> u64 {
        self.seen
            .insert(file_identity(path, metadata))
            .then_some(allocated_file_size(metadata))
            .unwrap_or(0)
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(not(unix))]
    Path(PathBuf),
}

fn file_identity(path: &Path, metadata: &fs::Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        let _ = path;
        FileIdentity::Unix { device: metadata.dev(), inode: metadata.ino() }
    }
    #[cfg(not(unix))]
    {
        FileIdentity::Path(path.to_path_buf())
    }
}

#[cfg(unix)]
fn allocated_file_size(metadata: &fs::Metadata) -> u64 {
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_file_size(metadata: &fs::Metadata) -> u64 {
    metadata.len()
}

pub(crate) fn get_dir_size(path: &Path) -> Result<u64> {
    SizeTracker::new().get_dir_size(path)
}

pub(crate) fn get_dir_size_bounded(
    path: &Path,
    byte_limit: u64,
    entry_limit: usize,
) -> Result<(u64, usize)> {
    SizeTracker::new().get_dir_size_bounded(path, byte_limit, entry_limit)
}

// Directory size measurement for cleanup reporting and session hints.
// Exports: exact and bounded recursive size scanners, hint limits.
// Deps: anyhow, std::fs, std::path.

use anyhow::Result;
use std::fs;
use std::path::Path;

pub(crate) const CLEANUP_HINT_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;
pub(crate) const CLEANUP_HINT_ENTRY_LIMIT: usize = 4096;

pub(crate) fn get_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            total += get_dir_size(&entry.path())?;
        } else if file_type.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

pub(crate) fn get_dir_size_bounded(
    path: &Path,
    byte_limit: u64,
    entry_limit: usize,
) -> Result<(u64, usize)> {
    let mut bytes = 0;
    let mut entries = 0;
    measure_dir_size_bounded(path, byte_limit, entry_limit, &mut bytes, &mut entries)?;
    Ok((bytes, entries))
}

fn measure_dir_size_bounded(
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
            measure_dir_size_bounded(&entry.path(), byte_limit, entry_limit, bytes, entries)?;
        } else if file_type.is_file() {
            *bytes += entry.metadata()?.len();
        }
        if *bytes >= byte_limit || *entries >= entry_limit {
            break;
        }
    }
    Ok(())
}

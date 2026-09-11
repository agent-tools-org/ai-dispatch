// Cleanup of terminal task homes with injectable resolution, removal, and warnings.
// Exports: clean_isolated_task_homes; deps: Store, SizeTracker, home_isolation.

use anyhow::Result;
use std::path::{Path, PathBuf};

use super::format_bytes;
use crate::store::Store;

pub(crate) fn clean_isolated_task_homes(
    store: &Store,
    dry_run: bool,
    sizes: &mut crate::cmd::clean_size::SizeTracker,
) -> Result<u64> {
    clean_isolated_task_homes_with(
        store, dry_run, sizes,
        crate::agent::home_isolation::resolve_real_home,
        crate::agent::home_isolation::remove_isolated_home,
        |warning| aid_warn!("{warning}"),
    )
}

pub(super) fn clean_isolated_task_homes_with(
    store: &Store,
    dry_run: bool,
    sizes: &mut crate::cmd::clean_size::SizeTracker,
    resolve_home: impl FnOnce() -> Result<PathBuf>,
    mut remove_home: impl FnMut(&Path, &Path) -> Result<()>,
    mut warn: impl FnMut(String),
) -> Result<u64> {
    let mut bytes = 0u64;
    let mut removed = 0usize;
    let real_home = if dry_run { None } else {
        match resolve_home() {
            Ok(home) => Some(home),
            Err(err) => {
                warn(format!("[aid] Warning: cannot resolve real HOME; isolated task homes remain: {err:#}"));
                None
            }
        }
    };
    for id in crate::cmd::clean_cargo_target::terminal_task_ids(store)? {
        let home_dir = crate::paths::task_dir(&id).join("home");
        if home_dir.exists() {
            let size = sizes.get_dir_size(&home_dir)?;
            if dry_run {
                println!("[dry-run] Would remove isolated task home for {} ({})", id, format_bytes(size));
                bytes += size;
            } else {
                let Some(real_home) = real_home.as_deref() else { continue };
                if let Err(err) = remove_home(&home_dir, real_home) {
                    warn(format!(
                        "[aid] Warning: failed to remove isolated task home for {} at '{}': {err:#}",
                        id,
                        home_dir.display()
                    ));
                    continue;
                }
                println!("Removed isolated task home for {} ({})", id, format_bytes(size));
                bytes += size;
            }
            removed += 1;
        }
    }
    if removed > 0 || dry_run {
        println!(
            "{} {removed} isolated task homes ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            format_bytes(bytes)
        );
    }
    Ok(bytes)
}

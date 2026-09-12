// Materializes private Cargo directories for remote-build login shells.
// Exports materialize to HOME construction; shares the embedded remote Cargo shim.
// Deps: std filesystem, Unix symlinks and executable permissions, anyhow.
use anyhow::{Context, Result};
use std::{fs, path::Path};

pub(super) fn materialize(real_cargo: &Path, isolated_cargo: &Path) -> Result<()> {
    fs::create_dir(isolated_cargo).with_context(|| {
        format!("cannot create private Cargo directory '{}'", isolated_cargo.display())
    })?;
    let bin = isolated_cargo.join("bin");
    fs::create_dir(&bin).context("cannot create private Cargo bin directory")?;
    link_entries(real_cargo, isolated_cargo, "bin")?;
    link_entries(&real_cargo.join("bin"), &bin, "cargo")?;
    let shim = bin.join("cargo");
    fs::write(&shim, include_str!("../remote_build/cargo.sh"))?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn link_entries(source: &Path, destination: &Path, excluded: &str) -> Result<()> {
    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("cannot read Cargo directory '{}'", source.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read entry in '{}'", source.display()))?;
        if entry.file_name() == excluded { continue; }
        #[cfg(unix)]
        std::os::unix::fs::symlink(entry.path(), destination.join(entry.file_name()))
            .with_context(|| format!("cannot link Cargo entry '{}'", entry.path().display()))?;
    }
    Ok(())
}

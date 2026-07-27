// Installs the release-managed official AID guide skill.
// Exports refresh(); depends on bundled default-skills files and std::fs.

use anyhow::{Context, Result};
use std::path::Path;

const GUIDE_FILES: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../default-skills/aid-guide/SKILL.md"),
    ),
    (
        "agents/openai.yaml",
        include_str!("../../default-skills/aid-guide/agents/openai.yaml"),
    ),
    (
        "references/command-index.md",
        include_str!("../../default-skills/aid-guide/references/command-index.md"),
    ),
    (
        "references/configuration.md",
        include_str!("../../default-skills/aid-guide/references/configuration.md"),
    ),
    (
        "references/dispatch.md",
        include_str!("../../default-skills/aid-guide/references/dispatch.md"),
    ),
    (
        "references/collaboration.md",
        include_str!("../../default-skills/aid-guide/references/collaboration.md"),
    ),
    (
        "references/task-operations.md",
        include_str!("../../default-skills/aid-guide/references/task-operations.md"),
    ),
    (
        "references/task-lifecycle.md",
        include_str!("../../default-skills/aid-guide/references/task-lifecycle.md"),
    ),
];

pub(super) fn refresh(skills_dir: &Path) -> Result<()> {
    let guide_dir = skills_dir.join("aid-guide");
    reject_symlink(&guide_dir)?;
    std::fs::create_dir_all(&guide_dir)?;
    for (relative_path, content) in GUIDE_FILES {
        write_managed_file(&guide_dir, relative_path, content)?;
    }
    println!("Updated official skill: {}", guide_dir.display());
    Ok(())
}

fn write_managed_file(root: &Path, relative_path: &str, content: &str) -> Result<()> {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        reject_symlink(parent)?;
        std::fs::create_dir_all(parent)?;
    }
    reject_symlink(&path)?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to update official skill file {}", path.display()))
}

fn reject_symlink(path: &Path) -> Result<()> {
    let Ok(metadata) = path.symlink_metadata() else {
        return Ok(());
    };
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "Refusing to update official skill through symlink {}",
        path.display()
    );
    Ok(())
}

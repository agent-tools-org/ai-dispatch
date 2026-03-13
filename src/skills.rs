// Skill loading for methodology prompt injection.
// Exports: load_skill(), load_skills(), list_skills().
// Deps: crate::paths, anyhow, std::fs.

use anyhow::{Context, Result};

fn skills_dir() -> std::path::PathBuf {
    crate::paths::aid_dir().join("skills")
}

pub fn load_skill(name: &str) -> Result<String> {
    let path = skills_dir().join(format!("{name}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("Skill '{name}' not found in ~/.aid/skills/")
        }
        Err(err) => Err(err).with_context(|| format!("Failed to read skill {}", path.display())),
    }
}

pub fn load_skills(names: &[String]) -> Result<String> {
    names
        .iter()
        .map(|name| load_skill(name))
        .collect::<Result<Vec<_>>>()
        .map(|skills| skills.join("\n\n"))
}

pub fn list_skills() -> Result<Vec<String>> {
    let dir = skills_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read skills dir {}", dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            skills.push(name.to_string());
        }
    }
    skills.sort();
    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_lists_skills_from_aid_home() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = skills_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test-writer.md"), "# Test Writer").unwrap();
        std::fs::write(dir.join("reviewer.md"), "# Reviewer").unwrap();

        assert_eq!(load_skill("test-writer").unwrap(), "# Test Writer");
        assert_eq!(list_skills().unwrap(), vec!["reviewer", "test-writer"]);
    }
}

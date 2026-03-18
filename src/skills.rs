// Skill loading for methodology prompt injection.
// Exports: load_skill(), resolve_skill_content(), load_skills(), list_skills(), auto_skills().
// Deps: crate::paths, crate::types, anyhow, std::fs.

use anyhow::{Context, Result};
use crate::types::AgentKind;
use crate::sanitize;

fn skills_dir() -> std::path::PathBuf {
    crate::paths::aid_dir().join("skills")
}

pub fn load_skill(name: &str) -> Result<String> {
    sanitize::validate_name(name, "skill")?;
    let path = skills_dir().join(format!("{name}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("Skill '{name}' not found in ~/.aid/skills/")
        }
        Err(err) => Err(err).with_context(|| format!("Failed to read skill {}", path.display())),
    }
}

pub fn resolve_skill_content(name: &str) -> Result<String> {
    load_skill(name)
}

pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

pub fn measure_skill_tokens(name: &str) -> Result<(String, usize)> {
    let content = load_skill(name)?;
    let tokens = estimate_tokens(&content);
    Ok((content, tokens))
}

#[allow(dead_code)]
pub fn compact_skill(content: &str, max_tokens: usize) -> String {
    if max_tokens == 0 {
        return "...".to_string();
    }
    if estimate_tokens(content) <= max_tokens {
        return content.to_string();
    }

    let mut collapsed_lines = Vec::new();
    let mut last_blank = false;
    for line in content.lines() {
        if line.trim().is_empty() {
            if last_blank {
                continue;
            }
            collapsed_lines.push(String::new());
            last_blank = true;
        } else {
            collapsed_lines.push(line.to_string());
            last_blank = false;
        }
    }

    let processed_lines: Vec<String> = collapsed_lines
        .into_iter()
        .map(|line| {
            if line.trim().is_empty() {
                return String::new();
            }
            let mut working = line.trim_start().to_string();
            working = working.trim_start_matches('#').trim_start().to_string();
            if let Some(stripped) = working.strip_prefix("- ") {
                working = stripped.to_string();
            }
            working.trim_start().to_string()
        })
        .collect();

    let compacted = processed_lines.join("\n");
    let char_limit = max_tokens.saturating_mul(4);
    if char_limit == 0 {
        return "...".to_string();
    }
    if compacted.len() <= char_limit {
        return compacted;
    }

    let mut end = 0;
    for (idx, ch) in compacted.char_indices() {
        if idx >= char_limit {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        return "...".to_string();
    }
    let truncated = &compacted[..end];
    format!("{truncated}...")
}

pub fn load_skills(names: &[String]) -> Result<String> {
    let mut contents = Vec::new();
    let mut total_tokens = 0usize;
    for name in names {
        let (content, tokens) = measure_skill_tokens(name)?;
        contents.push(content);
        total_tokens += tokens;
    }
    eprintln!("[aid] Skills loaded: {} skills, ~{} tokens", contents.len(), total_tokens);
    Ok(contents.join("\n\n"))
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

pub fn auto_skills(agent: &AgentKind, has_worktree: bool) -> Vec<String> {
    let _ = has_worktree;
    let available = list_skills().unwrap_or_default();
    let mut skills = Vec::new();
    match agent {
        AgentKind::Codex | AgentKind::OpenCode | AgentKind::Cursor | AgentKind::Kilo | AgentKind::Codebuff => {
            skills.push("implementer".to_string());
        }
        AgentKind::Gemini => {
            skills.push("researcher".to_string());
        }
        AgentKind::Custom => {}
    }
    skills.retain(|skill| available.iter().any(|available_skill| available_skill == skill));
    skills
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

    #[test]
    fn load_skill_rejects_invalid_name() {
        let err = load_skill("../escape").unwrap_err();
        assert!(err.to_string().contains("Invalid skill name"));
    }

    #[test]
    fn auto_skills_returns_agent_defaults_when_installed() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = skills_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();
        std::fs::write(dir.join("researcher.md"), "# Researcher").unwrap();

        assert_eq!(auto_skills(&AgentKind::Codex, false), vec!["implementer"]);
        assert_eq!(auto_skills(&AgentKind::OpenCode, false), vec!["implementer"]);
        assert_eq!(auto_skills(&AgentKind::Cursor, true), vec!["implementer"]);
        assert_eq!(auto_skills(&AgentKind::Gemini, false), vec!["researcher"]);
        assert_eq!(auto_skills(&AgentKind::Kilo, false), vec!["implementer"]);
    }

    #[test]
    fn auto_skills_skips_missing_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = skills_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

        assert!(auto_skills(&AgentKind::Gemini, false).is_empty());
    }

    #[test]
    fn estimate_tokens_uses_length_divided_by_four() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abc"), 0);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn compact_skill_truncates_and_cleans_content() {
        let input = "\n# Topic\n\n- bullet one\n- bullet two\nDetail line\nMore detail\n";
        let compacted = compact_skill(input, 2);
        assert!(compacted.contains("Topic"));
        assert!(!compacted.contains('#'));
        assert!(!compacted.contains("- "));
        assert!(compacted.ends_with("..."));
    }

    #[test]
    fn compact_skill_returns_same_when_under_limit() {
        let input = "ok";
        assert_eq!(compact_skill(input, 1), input);
    }
}

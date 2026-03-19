// Handler for `aid changelog` — render git tag history and tagged commits.
// Exports: `run`; deps: anyhow, std::process::Command.

use anyhow::{bail, Result};
use std::ffi::OsStr;
use std::process::Command;

const EMBEDDED_CHANGELOG: &str = env!("AID_CHANGELOG");

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    tag: String,
    date: String,
    commits: Vec<String>,
}

pub(crate) fn run(version: Option<String>, all: bool, count: usize, git: bool) -> Result<()> {
    let embedded = embedded_decoded();

    if git {
        return run_from_git_tags(version, all, count);
    }

    if !embedded.is_empty() {
        if let Some(v) = version.as_deref() {
            if let Some(section) = embedded_section_for_version(&embedded, v) {
                print!("{section}");
                return Ok(());
            }
        } else if all {
            print!("{embedded}");
            return Ok(());
        } else {
            let lines = embedded.lines().collect::<Vec<_>>();
            let version_starts: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.starts_with("## v"))
                .map(|(i, _)| i)
                .collect();
            let limit = count.min(version_starts.len());
            let start_idx = version_starts.first().copied().unwrap_or(0);
            let end_idx = version_starts.get(limit).copied().unwrap_or(lines.len());
            for line in &lines[start_idx..end_idx] {
                println!("{line}");
            }
            return Ok(());
        }
    }

    // Only fall back to git tags when --git is explicitly passed.
    // Without --git, we show only aid's embedded changelog to avoid
    // displaying another repo's history when run outside the aid repo.
    println!("No embedded changelog available. Use --git to show tags from the current repo.");
    Ok(())
}

fn run_from_git_tags(version: Option<String>, all: bool, count: usize) -> Result<()> {
    let tags = version_tags();

    if tags.is_empty() {
        return Ok(());
    }

    let indexes = selected_indexes(&tags, version.as_deref(), all, count)?;
    let text = render_entries(&build_entries(&tags, &indexes)?);
    if !text.is_empty() {
        print!("{text}");
    }
    Ok(())
}

fn embedded_decoded() -> String {
    // build.rs escapes newlines as a sentinel token to keep `cargo:rustc-env` values single-line.
    EMBEDDED_CHANGELOG.replace("__AID_NL__", "\n")
}

fn embedded_section_for_version<'a>(embedded: &'a str, version: &str) -> Option<&'a str> {
    let wanted = version.trim_start_matches('v');
    let prefix = format!("## v{wanted} (");
    let start = embedded.find(&prefix)?;
    let search_from = start + prefix.len();
    let end = embedded[search_from..]
        .find("\n## ")
        .map(|i| search_from + i)
        .unwrap_or(embedded.len());
    Some(&embedded[start..end])
}

fn version_tags() -> Vec<String> {
    git(["tag", "--sort=-version:refname"])
        .unwrap_or_default()
        .lines()
        .filter(|tag| is_version_tag(tag))
        .map(str::to_string)
        .collect()
}

fn selected_indexes(
    tags: &[String],
    version: Option<&str>,
    all: bool,
    count: usize,
) -> Result<Vec<usize>> {
    if let Some(version) = version {
        let wanted = version.trim_start_matches('v');
        let Some(index) = tags
            .iter()
            .position(|tag| tag.trim_start_matches('v') == wanted)
        else {
            bail!("Version '{version}' not found");
        };
        return Ok(vec![index]);
    }
    if all {
        return Ok((0..tags.len()).collect());
    }
    Ok((0..tags.len().min(count)).collect())
}

fn build_entries(tags: &[String], indexes: &[usize]) -> Result<Vec<Entry>> {
    indexes
        .iter()
        .map(|&index| {
            let tag = &tags[index];
            let commits = commit_messages(tag, tags.get(index + 1).map(String::as_str))?;
            Ok(Entry {
                tag: tag.clone(),
                date: git(["log", "-1", "--format=%ci", tag])?
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string(),
                commits,
            })
        })
        .collect()
}

fn commit_messages(tag: &str, previous_tag: Option<&str>) -> Result<Vec<String>> {
    let range = previous_tag.map_or_else(|| tag.to_string(), |prev| format!("{prev}..{tag}"));
    let commits: Vec<String> = git(["log", "--no-merges", "--format=%s", &range])?
        .lines()
        .map(str::to_string)
        .collect();
    Ok(if commits.is_empty() {
        vec!["No commits found".to_string()]
    } else {
        commits
    })
}

fn git<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_version_tag(tag: &str) -> bool {
    let mut parts = tag.strip_prefix('v').unwrap_or("").split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), Some(c), None)
            if a.chars().all(|ch| ch.is_ascii_digit())
                && b.chars().all(|ch| ch.is_ascii_digit())
                && c.chars().all(|ch| ch.is_ascii_digit())
    )
}

fn render_entries(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|entry| {
            let commits = entry
                .commits
                .iter()
                .map(|commit| format!("- {commit}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("## {} ({})\n{}\n", entry.tag, entry.date, commits)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{embedded_section_for_version, render_entries, selected_indexes, Entry};

    #[test]
    fn renders_version_sections() {
        let text = render_entries(&[Entry {
            tag: "v8.22.0".to_string(),
            date: "2026-03-19".to_string(),
            commits: vec![
                "Add changelog command".to_string(),
                "Wire CLI dispatch".to_string(),
            ],
        }]);
        assert_eq!(
            text,
            "## v8.22.0 (2026-03-19)\n- Add changelog command\n- Wire CLI dispatch\n"
        );
    }

    #[test]
    fn selects_specific_version_without_v_prefix() {
        let tags = vec!["v8.22.0".to_string(), "v8.21.14".to_string()];
        assert_eq!(
            selected_indexes(&tags, Some("8.21.14"), false, 5).unwrap(),
            vec![1]
        );
    }

    #[test]
    fn extracts_embedded_section_for_version() {
        let embedded = concat!(
            "## v1.2.3 (2026-01-01)\n- A\n",
            "\n",
            "## v0.1.0 (2026-01-02)\n- B\n"
        );
        assert_eq!(
            embedded_section_for_version(embedded, "1.2.3"),
            Some("## v1.2.3 (2026-01-01)\n- A\n")
        );
        assert_eq!(
            embedded_section_for_version(embedded, "v0.1.0"),
            Some("## v0.1.0 (2026-01-02)\n- B\n")
        );
        assert_eq!(embedded_section_for_version(embedded, "9.9.9"), None);
    }
}

// Handler for `aid changelog` — render git tag history and tagged commits.
// Exports: `run`; deps: anyhow, std::process::Command.

use anyhow::{Result, bail};
use std::ffi::OsStr;
use std::process::Command;

const EMBEDDED_CHANGELOG: &str = env!("AID_CHANGELOG");
const NO_VERSION_TAGS_MESSAGE: &str =
    "No version tags found. Run from the ai-dispatch repo or rebuild aid.";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    tag: String,
    date: String,
    commits: Vec<String>,
}

pub(crate) fn run(version: Option<String>, all: bool, count: usize) -> Result<()> {
    let tags = version_tags()?;
    let embedded = if tags.is_empty() || version.is_some() {
        embedded_decoded()
    } else {
        String::new()
    };

    if tags.is_empty() && version.is_none() {
        if let Some(text) = no_version_tags_fallback(&embedded) {
            print!("{text}");
        }
        return Ok(());
    }

    let indexes = match selected_indexes(&tags, version.as_deref(), all, count) {
        Ok(indexes) => indexes,
        Err(err) => {
            if let Some(version) = version.as_deref() {
                if let Some(section) = embedded_section_for_version(&embedded, version) {
                    print!("{section}");
                    return Ok(());
                }
            }
            return Err(err);
        }
    };

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

fn no_version_tags_fallback(embedded: &str) -> Option<String> {
    if embedded.is_empty() {
        Some(NO_VERSION_TAGS_MESSAGE.to_string())
    } else {
        Some(embedded.to_string())
    }
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

fn version_tags() -> Result<Vec<String>> {
    Ok(git(["tag", "--sort=-version:refname"])?
        .lines()
        .filter(|tag| is_version_tag(tag))
        .map(str::to_string)
        .collect())
}

fn selected_indexes(tags: &[String], version: Option<&str>, all: bool, count: usize) -> Result<Vec<usize>> {
    if let Some(version) = version {
        let wanted = version.trim_start_matches('v');
        let Some(index) = tags.iter().position(|tag| tag.trim_start_matches('v') == wanted) else {
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
    let commits: Vec<String> = git(["log", &range, "--oneline"])?
        .lines()
        .filter_map(|line| line.split_once(' ').map(|(_, message)| message.to_string()))
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
    use super::{
        embedded_section_for_version, no_version_tags_fallback, Entry, render_entries,
        selected_indexes,
    };

    #[test]
    fn renders_version_sections() {
        let text = render_entries(&[Entry {
            tag: "v8.22.0".to_string(),
            date: "2026-03-19".to_string(),
            commits: vec!["Add changelog command".to_string(), "Wire CLI dispatch".to_string()],
        }]);
        assert_eq!(
            text,
            "## v8.22.0 (2026-03-19)\n- Add changelog command\n- Wire CLI dispatch\n"
        );
    }

    #[test]
    fn selects_specific_version_without_v_prefix() {
        let tags = vec!["v8.22.0".to_string(), "v8.21.14".to_string()];
        assert_eq!(selected_indexes(&tags, Some("8.21.14"), false, 5).unwrap(), vec![1]);
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

    #[test]
    fn no_version_tags_message_when_embedded_empty() {
        assert_eq!(
            no_version_tags_fallback(""),
            Some("No version tags found. Run from the ai-dispatch repo or rebuild aid.".to_string())
        );
    }
}

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
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

fn build_embedded_changelog() -> Option<String> {
    let tags = git(&["tag", "--sort=-version:refname"])?
        .lines()
        .filter(|t| is_version_tag(t))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let count = tags.len().min(10);
    let mut sections = Vec::with_capacity(count);
    for i in 0..count {
        let tag = &tags[i];
        let prev = tags.get(i + 1).map(String::as_str);
        let range = prev.map_or_else(|| tag.to_string(), |p| format!("{p}..{tag}"));

        let commits_out = git(&["log", "--no-merges", "--format=%s", &range])?;
        let commits = commits_out.lines().map(str::to_string).collect::<Vec<_>>();
        let commits = if commits.is_empty() {
            vec!["No commits found".to_string()]
        } else {
            commits
        };

        let date_out = git(&["log", "-1", "--format=%ci", tag])?;
        let date = date_out.split_whitespace().next().unwrap_or("").to_string();
        let commits_text = commits
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n");

        sections.push(format!("## {} ({})\n{}\n", tag, date, commits_text));
    }
    Some(sections.join("\n"))
}

fn escape_newlines(s: &str) -> String {
    s.replace('\n', "__AID_NL__")
}

fn read_changelog_file() -> Option<String> {
    let content = std::fs::read_to_string("CHANGELOG.md").ok()?;
    if content.trim().is_empty() { None } else { Some(content) }
}

/// Commit identity + dirty flag for `aid --version`. Falls back to a plain
/// "no git metadata" marker (never a blank field) when there is no git repo
/// to inspect, e.g. a crates.io tarball or released source archive.
fn build_git_info() -> String {
    git(&["describe", "--always", "--dirty"])
        .filter(|desc| !desc.is_empty())
        .unwrap_or_else(|| "no git metadata".to_string())
}

fn main() {
    // Watch git locations so changelog re-embeds when tags change.
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=CHANGELOG.md");
    // Watch HEAD/index so the embedded commit SHA and dirty flag stay current.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    // Prefer live git tags; fall back to static CHANGELOG.md (for crates.io installs).
    let text = build_embedded_changelog()
        .or_else(read_changelog_file)
        .unwrap_or_default();
    println!("cargo:rustc-env=AID_CHANGELOG={}", escape_newlines(&text));
    println!("cargo:rustc-env=AID_GIT_INFO={}", build_git_info());
}

// Explore file detection helpers for prompt-driven context selection.
// Exports auto_detect_files() using simple path token, directory, and wildcard rules.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const KNOWN_EXTENSIONS: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".toml", ".json", ".md",
];

pub fn auto_detect_files(prompt: &str, project_root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();

    for raw_token in prompt.split_whitespace() {
        let token = clean_token(raw_token);
        if token.is_empty() || !looks_like_path(&token) {
            continue;
        }
        collect_matches(&token, project_root, &mut found, &mut seen);
    }

    found
}

fn collect_matches(
    token: &str,
    project_root: &Path,
    found: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    if token.contains('*') {
        for path in expand_glob(token, project_root) {
            push_path(path, project_root, found, seen);
        }
        return;
    }

    let candidate = project_root.join(token);
    if candidate.is_file() {
        push_path(candidate, project_root, found, seen);
    } else if candidate.is_dir() {
        for path in collect_directory_files(&candidate) {
            push_path(path, project_root, found, seen);
        }
    }
}

fn clean_token(raw_token: &str) -> String {
    raw_token
        .trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'))
        .to_string()
}

fn looks_like_path(token: &str) -> bool {
    token.contains('/')
        || token.contains('*')
        || KNOWN_EXTENSIONS.iter().any(|ext| token.ends_with(ext))
}

fn push_path(
    path: PathBuf,
    project_root: &Path,
    found: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    if let Ok(relative) = path.strip_prefix(project_root) {
        let display = relative.to_string_lossy().to_string();
        if seen.insert(display.clone()) {
            found.push(display);
        }
    }
}

fn collect_directory_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_files(dir, &mut files);
    files
}

fn visit_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

fn expand_glob(pattern: &str, project_root: &Path) -> Vec<PathBuf> {
    let search_root = glob_search_root(pattern, project_root);
    let mut candidates = Vec::new();
    visit_files(&search_root, &mut candidates);
    candidates
        .into_iter()
        .filter(|path| {
            path.strip_prefix(project_root)
                .ok()
                .and_then(|relative| relative.to_str())
                .is_some_and(|relative| wildcard_matches(pattern, relative))
        })
        .collect()
}

fn glob_search_root(pattern: &str, project_root: &Path) -> PathBuf {
    let prefix = pattern.split('*').next().unwrap_or_default();
    if prefix.is_empty() {
        return project_root.to_path_buf();
    }

    let prefix_path = project_root.join(prefix);
    if prefix_path.is_dir() {
        prefix_path
    } else {
        prefix_path.parent().unwrap_or(project_root).to_path_buf()
    }
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    wildcard_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn wildcard_match_bytes(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    if pattern[0] == b'*' {
        return wildcard_match_bytes(&pattern[1..], text)
            || (!text.is_empty() && wildcard_match_bytes(pattern, &text[1..]));
    }
    if text.is_empty() || pattern[0] != text[0] {
        return false;
    }
    wildcard_match_bytes(&pattern[1..], &text[1..])
}

#[cfg(test)]
mod tests {
    use super::auto_detect_files;
    use tempfile::TempDir;

    #[test]
    fn finds_explicit_files() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("src/lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "pub fn demo() {}").unwrap();

        let files = auto_detect_files("inspect src/lib.rs", temp.path());
        assert_eq!(files, vec!["src/lib.rs"]);
    }

    #[test]
    fn expands_directories_and_globs() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src/agent")).unwrap();
        std::fs::write(temp.path().join("src/agent/mod.rs"), "").unwrap();
        std::fs::write(temp.path().join("src/agent/codex.rs"), "").unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "").unwrap();

        let files = auto_detect_files("review src/agent/ and src/*.rs", temp.path());
        assert!(files.contains(&"src/agent/mod.rs".to_string()));
        assert!(files.contains(&"src/agent/codex.rs".to_string()));
        assert!(files.contains(&"src/main.rs".to_string()));
    }
}

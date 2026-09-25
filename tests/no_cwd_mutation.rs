// Guard: no Rust source under src/ or tests/ may change the process working directory.
// The cwd is process-global; a test that moves it races every parallel test and child process.
// Deps: std only.

use std::fs;
use std::path::{Path, PathBuf};

// Split so this file does not match its own pattern.
const FORBIDDEN: &str = concat!("set_", "current_dir");

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|err| panic!("read {}: {err}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_changes_the_process_working_directory() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("tests"), &mut files);
    assert!(files.len() > 10, "source scan found too few files: {}", files.len());
    let offenders: Vec<String> = files
        .iter()
        .filter_map(|path| {
            let text = fs::read_to_string(path).ok()?;
            text.lines().enumerate().find(|(_, line)| line.contains(FORBIDDEN)).map(|(idx, _)| {
                format!("{}:{}", path.strip_prefix(root).unwrap_or(path).display(), idx + 1)
            })
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "{FORBIDDEN} mutates process-global state; pass explicit directories instead: {offenders:?}"
    );
}

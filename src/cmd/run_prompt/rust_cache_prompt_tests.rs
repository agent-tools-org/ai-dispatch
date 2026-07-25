// Tests for Rust build-cache prompt injection.
// Exports: none. Deps: run_prompt bundle builder, in-memory Store, tempfile.

use super::*;

fn prompt_args(dir: Option<String>) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Update the code".to_string(),
        dir,
        ..Default::default()
    }
}

#[test]
fn build_prompt_bundle_includes_rust_cache_line_for_rust_project() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let store = Store::open_memory().unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &prompt_args(Some(temp.path().to_string_lossy().to_string())),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(bundle.effective_prompt.starts_with(
        "Rust project: CARGO_TARGET_DIR already points at a warm shared target directory; do not override it."
    ));
}

#[test]
fn build_prompt_bundle_omits_rust_cache_line_for_non_rust_project() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &prompt_args(Some(temp.path().to_string_lossy().to_string())),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("CARGO_TARGET_DIR already points"));
}

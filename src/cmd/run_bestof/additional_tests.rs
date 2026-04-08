// Extra best-of tests kept separate to preserve small test files.
// Covers metric cwd fallback and plan expansion behavior.

use super::*;

#[test]
fn expand_best_of_plan_cycles_available_agents() {
    let plan = expand_best_of_plan(vec![AgentKind::Codex, AgentKind::Gemini], 5);
    assert_eq!(
        plan,
        vec![
            AgentKind::Codex,
            AgentKind::Gemini,
            AgentKind::Codex,
            AgentKind::Gemini,
            AgentKind::Codex,
        ]
    );
}

#[test]
fn evaluate_metric_uses_repo_path_when_worktree_is_absent() {
    let repo_dir = std::env::temp_dir().join(format!(
        "aid-bestof-metric-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&repo_dir).unwrap();
    let marker = repo_dir.join("metric-marker");
    std::fs::write(&marker, "ok").unwrap();
    let score = evaluate_metric(
        "[ -f metric-marker ] && echo 1 || echo 0",
        None,
        Some(repo_dir.to_str().unwrap()),
    );
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_dir(&repo_dir).unwrap();
    assert_eq!(score, Some(1.0));
}

#[test]
fn evaluate_metric_falls_back_to_repo_path_when_worktree_is_stale() {
    let repo_dir = std::env::temp_dir().join(format!(
        "aid-bestof-metric-repo-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&repo_dir).unwrap();
    let marker = repo_dir.join("metric-marker");
    std::fs::write(&marker, "ok").unwrap();
    let stale = repo_dir.join("missing-worktree");
    let score = evaluate_metric(
        "[ -f metric-marker ] && echo 1 || echo 0",
        Some(stale.to_str().unwrap()),
        Some(repo_dir.to_str().unwrap()),
    );
    std::fs::remove_file(&marker).unwrap();
    std::fs::remove_dir(&repo_dir).unwrap();
    assert_eq!(score, Some(1.0));
}

// Extra best-of tests kept separate to preserve small test files.
// Covers metric cwd fallback and plan expansion behavior.

use super::*;
use super::output_files::{
    DispatchArtifacts, dispatch_artifacts_for_candidate, finalize_winner_artifacts, suffixed_path,
};
use tempfile::tempdir;

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

#[test]
fn suffixed_path_inserts_suffix_before_extension() {
    assert_eq!(suffixed_path("result.md", "-bo2"), "result-bo2.md");
    assert_eq!(suffixed_path("output", "-bo2"), "output-bo2");
    assert_eq!(suffixed_path("nested/output.json", "-bo3"), "nested/output-bo3.json");
}

#[test]
fn candidate_artifacts_use_unique_paths_after_first_run() {
    let primary = dispatch_artifacts_for_candidate(Some("output.json"), Some("result.md"), 0);
    let secondary = dispatch_artifacts_for_candidate(Some("output.json"), Some("result.md"), 1);
    assert_eq!(primary.output.as_deref(), Some("output.json"));
    assert_eq!(primary.result_file.as_deref(), Some("result.md"));
    assert_eq!(secondary.output.as_deref(), Some("output-bo2.json"));
    assert_eq!(secondary.result_file.as_deref(), Some("result-bo2.md"));
}

#[test]
fn finalize_winner_artifacts_copies_winner_and_cleans_loser() {
    let temp = tempdir().unwrap();
    let original_output = temp.path().join("output.json");
    let original_result = temp.path().join("result.md");
    let winner_output = temp.path().join("output-bo2.json");
    let winner_result = temp.path().join("result-bo2.md");
    let loser_output = temp.path().join("output-bo3.json");
    let loser_result = temp.path().join("result-bo3.md");
    std::fs::write(&winner_output, "winner output").unwrap();
    std::fs::write(&winner_result, "winner result").unwrap();
    std::fs::write(&loser_output, "loser output").unwrap();
    std::fs::write(&loser_result, "loser result").unwrap();
    let original = dispatch_artifacts_for_candidate(
        Some(original_output.to_str().unwrap()),
        Some(original_result.to_str().unwrap()),
        0,
    );
    let winner_id = TaskId("t-winner".into());
    let loser_id = TaskId("t-loser".into());
    let candidates = vec![
        (
            winner_id.clone(),
            DispatchArtifacts {
                output: Some(winner_output.to_str().unwrap().to_string()),
                result_file: Some(winner_result.to_str().unwrap().to_string()),
            },
        ),
        (
            loser_id,
            DispatchArtifacts {
                output: Some(loser_output.to_str().unwrap().to_string()),
                result_file: Some(loser_result.to_str().unwrap().to_string()),
            },
        ),
    ];
    finalize_winner_artifacts(&original, &candidates, &winner_id).unwrap();
    assert_eq!(std::fs::read_to_string(&original_output).unwrap(), "winner output");
    assert_eq!(std::fs::read_to_string(&original_result).unwrap(), "winner result");
    assert!(!loser_output.exists());
    assert!(!loser_result.exists());
}

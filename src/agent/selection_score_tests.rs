// Regression coverage for behavior-preserving selector score decomposition.
// Proves score_for retains the pre-refactor floating-point bit pattern.
// Deps: selection scoring, classifier profile, team config, isolated AID_HOME.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use tempfile::TempDir;

use super::selection_quota::{headroom_penalty, penalty_from_used};
use super::selection_scoring::{
    CandidateContext, model_capability_score, model_quality_score, score_breakdown, score_for,
};
use super::advise;
use crate::agent::classifier::{Complexity, TaskCategory, TaskProfile};
use crate::live_quota::CacheDirGuard;
use crate::paths::AidHomeGuard;
use crate::team::TeamConfig;
use crate::types::{
    AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency,
};

fn score_ctx<'a>(
    profile: &'a TaskProfile,
    history_map: &'a HashMap<AgentKind, (f64, usize)>,
    avg_cost_map: &'a HashMap<AgentKind, f64>,
    team: Option<&'a TeamConfig>,
    penalize_rate_limit: bool,
) -> CandidateContext<'a> {
    CandidateContext {
        profile,
        team,
        history_map,
        avg_cost_map,
        team_default: None,
        budget: false,
        declared_budget: None,
        penalize_rate_limit,
    }
}

fn isolated() -> (TempDir, PathBuf, AidHomeGuard, CacheDirGuard) {
    let temp = TempDir::new().expect("temp dir");
    let home = AidHomeGuard::set(temp.path());
    let cache = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache).expect("cache dir");
    let guard = CacheDirGuard::set(&cache);
    (temp, cache, home, guard)
}

fn write_snapshot(cache: &Path, provider: &str, used: f64, age_secs: i64) {
    let fetched = Utc::now() - ChronoDuration::seconds(age_secs);
    let fetched_at = fetched.to_rfc3339_opts(SecondsFormat::Secs, true);
    let body = format!(
        r#"{{"ok":true,"snapshot":{{"provider":"{provider}","windows":[{{"label":"5h","used_percent":{used},"resets_at":"2026-08-18T00:55:28Z"}}],"fetched_at":"{fetched_at}"}}}}"#
    );
    std::fs::write(cache.join(format!("{provider}.json")), body).expect("snapshot");
}

fn write_clock_hold(home: &Path, agent: &str) {
    let future = (chrono::Local::now().naive_local() + ChronoDuration::days(1))
        .format("%b %d, %Y %I:%M %p")
        .to_string();
    std::fs::write(
        home.join(format!("rate-limit-{agent}")),
        format!("recovery_at: {future}\nmessage: quota exhausted\n"),
    )
    .expect("marker");
}

fn free_profile() -> TaskProfile {
    TaskProfile {
        category: TaskCategory::SimpleEdit,
        complexity: Complexity::Low,
    }
}

fn declared(urgency: TaskUrgency) -> DeclaredTaskProfile {
    DeclaredTaskProfile {
        difficulty: TaskDifficulty::Simple,
        budget: TaskBudget::Free,
        urgency,
        rigor: TaskRigor::Standard,
    }
}

#[test]
fn score_for_is_bit_identical_to_pre_breakdown_value() {
    let temp = TempDir::new().expect("temp dir");
    let _guard = AidHomeGuard::set(temp.path());
    let profile = TaskProfile {
        category: TaskCategory::ComplexImpl,
        complexity: Complexity::High,
    };
    let history_map = HashMap::from([(AgentKind::Codex, (0.9, 10))]);
    let avg_cost_map = HashMap::new();
    let team = TeamConfig {
        id: "regression".to_string(),
        display_name: "Regression".to_string(),
        description: String::new(),
        preferred_agents: vec!["codex".to_string()],
        default_agent: None,
        overrides: HashMap::new(),
        rules: Vec::new(),
        toolbox: Default::default(),
    };
    let context = score_ctx(&profile, &history_map, &avg_cost_map, Some(&team), true);

    let score = score_for(&context, AgentKind::Codex);
    let breakdown = score_breakdown(&context, AgentKind::Codex);

    // Absolute pin, so an unintended scoring change cannot slip through: floating
    // addition is not associative and a reordered sum can flip a tie silently.
    // It is derived from the model catalog, so a legitimate catalog refresh moves
    // it — re-pin deliberately and say why. 2026-08-05: 16.3 -> 16.35 when the
    // refresh made gpt-5.6-sol codex's default.
    assert_eq!(score.to_bits(), 0x4030_5999_9999_999a);
    assert_eq!(breakdown.total.to_bits(), score.to_bits());
    assert_eq!(breakdown.headroom_penalty, 0.0);
}

#[test]
fn discovered_agy_model_keeps_base_score_when_capability_is_unknown() {
    let temp = TempDir::new().expect("temp dir");
    let _home = AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("aid dirs");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let cache = serde_json::json!({
        "agy": {"models": ["gemini-3.7-flash-high"], "updated_at_secs": now}
    });
    std::fs::write(
        crate::paths::aid_dir().join("served_models_cache.json"),
        cache.to_string(),
    )
    .expect("served-model cache");

    let capability = model_capability_score(AgentKind::Antigravity, "gemini-3.7-flash-high");
    assert_eq!(capability, None);
    assert_eq!(model_quality_score(8, capability), 8.0);
}

#[test]
fn headroom_schedule_never_boosts() {
    assert_eq!(penalty_from_used(0.0), 0.0);
    assert_eq!(penalty_from_used(49.9), 0.0);
    assert_eq!(penalty_from_used(50.0), -1.0);
    assert_eq!(penalty_from_used(79.9), -1.0);
    assert_eq!(penalty_from_used(80.0), -3.0);
    assert_eq!(penalty_from_used(94.9), -3.0);
    assert_eq!(penalty_from_used(95.0), -6.0);
    assert_eq!(penalty_from_used(100.0), -6.0);
}

#[test]
fn two_free_agents_ten_percent_ranks_three_above_ninety() {
    let (_temp, cache, _home, _guard) = isolated();
    let profile = free_profile();
    let history = HashMap::new();
    let costs = HashMap::new();
    let ctx = score_ctx(&profile, &history, &costs, None, true);
    let qwen_base = score_for(&ctx, AgentKind::Qwen);
    let agy_base = score_for(&ctx, AgentKind::Antigravity);

    write_snapshot(&cache, "qwen", 90.0, 60);
    write_snapshot(&cache, "agy", 10.0, 60);

    let qwen = score_breakdown(&ctx, AgentKind::Qwen);
    let agy = score_breakdown(&ctx, AgentKind::Antigravity);
    assert_eq!(qwen.headroom_penalty, -3.0);
    assert_eq!(agy.headroom_penalty, 0.0);
    assert_eq!(qwen.total, qwen_base - 3.0);
    assert_eq!(agy.total, agy_base);
    assert_eq!(agy.total - qwen.total, (agy_base - qwen_base) + 3.0);
}

#[test]
fn stale_snapshot_does_not_retune_score() {
    let (_temp, cache, _home, _guard) = isolated();
    let profile = free_profile();
    let history = HashMap::new();
    let costs = HashMap::new();
    let ctx = score_ctx(&profile, &history, &costs, None, true);
    let baseline = score_for(&ctx, AgentKind::Qwen);
    write_snapshot(&cache, "qwen", 90.0, 20 * 60);
    let breakdown = score_breakdown(&ctx, AgentKind::Qwen);
    assert_eq!(headroom_penalty(AgentKind::Qwen), 0.0);
    assert_eq!(breakdown.headroom_penalty, 0.0);
    assert_eq!(breakdown.total.to_bits(), baseline.to_bits());
}

#[test]
fn unused_quota_does_not_boost() {
    let (_temp, cache, _home, _guard) = isolated();
    let profile = free_profile();
    let history = HashMap::new();
    let costs = HashMap::new();
    let ctx = score_ctx(&profile, &history, &costs, None, true);
    let baseline = score_for(&ctx, AgentKind::Qwen);
    write_snapshot(&cache, "qwen", 10.0, 60);
    let breakdown = score_breakdown(&ctx, AgentKind::Qwen);
    assert_eq!(breakdown.headroom_penalty, 0.0);
    assert_eq!(breakdown.total.to_bits(), baseline.to_bits());
}

#[test]
fn held_uses_rate_limit_penalty_not_headroom() {
    let (temp, cache, _home, _guard) = isolated();
    write_clock_hold(temp.path(), "qwen");
    write_snapshot(&cache, "qwen", 90.0, 60);
    let profile = free_profile();
    let history = HashMap::new();
    let costs = HashMap::new();
    let ctx = score_ctx(&profile, &history, &costs, None, true);
    let breakdown = score_breakdown(&ctx, AgentKind::Qwen);
    assert_eq!(breakdown.rate_limit_penalty, -10.0);
    assert_eq!(breakdown.headroom_penalty, 0.0);
}

#[test]
fn held_background_keeps_zero_penalty_and_note_says_wait() {
    let (temp, _cache, _home, _guard) = isolated();
    write_clock_hold(temp.path(), "codex");
    let report = advise(
        "add a null check",
        declared(TaskUrgency::Background),
        Some(TaskCategory::SimpleEdit),
        None,
        None,
        0,
    );
    let codex = report
        .candidates
        .iter()
        .find(|item| item.agent == "codex")
        .expect("codex candidate");
    assert_eq!(codex.breakdown.rate_limit_penalty, 0.0);
    assert_eq!(codex.quota.status, "held");
    assert!(
        report.notes.iter().any(|note| note.contains("codex") && note.contains("wait")),
        "notes: {:?}",
        report.notes
    );
}

#[test]
fn advise_notes_distinguish_held_degraded_and_skipped() {
    let (temp, cache, _home, _guard) = isolated();
    write_clock_hold(temp.path(), "codex");
    write_snapshot(&cache, "qwen", 90.0, 60);
    let report = advise(
        "add a null check",
        declared(TaskUrgency::Normal),
        Some(TaskCategory::SimpleEdit),
        None,
        None,
        0,
    );
    let qwen = report
        .candidates
        .iter()
        .find(|item| item.agent == "qwen")
        .expect("qwen");
    assert_eq!(qwen.quota.status, "degraded");
    assert_eq!(qwen.quota.used_percent, Some(90.0));
    assert_eq!(qwen.quota.source, "probe");
    assert_eq!(qwen.quota.wall, "none");
    assert!(!qwen.quota.stale);
    assert!(qwen.quota.freshness_secs.is_some());
    assert!(qwen.quota.resets_at.is_some());
    assert!(
        report.notes.iter().any(|note| note.contains("qwen") && note.contains("degraded")),
        "notes: {:?}",
        report.notes
    );
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.contains("codex") && note.contains("held") && note.contains("skipped")),
        "notes: {:?}",
        report.notes
    );
}

// Tests for per-CLI quota signatures. Every message below is verbatim captured
// CLI output, not an invented shape — inventing the envelope is what let the
// previous adapter fix pass its own tests while doing nothing in production.

use super::*;

#[test]
fn qwen_token_plan_exhaustion_is_recognized() {
    let msg = "Quota exhausted: Your token-plan 5-hour quota has been exhausted. The quota will reset later.";
    let (agent, minutes) = match_quota_signature(msg).expect("qwen quota message must match");
    assert_eq!(agent, AgentKind::Qwen);
    assert_eq!(minutes, 300);
}

#[test]
fn droid_weekly_limit_is_recognized_with_a_day_long_cooldown() {
    let msg = "402 You've reached your weekly standard usage limit (resets in 1 day).\nSwitch to Droid Core or enable Extra Usage to continue.";
    let (agent, minutes) = match_quota_signature(msg).expect("droid quota message must match");
    assert_eq!(agent, AgentKind::Droid);
    assert_eq!(minutes, 1440);
}

#[test]
fn codex_usage_limit_is_recognized() {
    let msg = "You have hit your usage limit. Upgrade to Pro or try again at Mar 19th, 2026 2:27 PM.";
    let (agent, _) = match_quota_signature(msg).expect("codex quota message must match");
    assert_eq!(agent, AgentKind::Codex);
}

#[test]
fn ordinary_output_does_not_match_any_signature() {
    assert!(match_quota_signature("compiling ai-dispatch v10.0.0").is_none());
    assert!(match_quota_signature("network timeout after 30s").is_none());
}

#[test]
fn resets_in_one_day_parses_to_roughly_a_day_out() {
    let parsed = parse_relative_recovery("resets in 1 day").expect("must parse");
    let delta = parsed - Local::now().naive_local();
    assert!(delta.num_hours() >= 23 && delta.num_hours() <= 24, "got {delta:?}");
}

#[test]
fn resets_in_minutes_and_hours_parse() {
    let minutes = parse_relative_recovery("please wait, resets in 45 minutes").expect("minutes");
    assert!((minutes - Local::now().naive_local()).num_minutes() >= 44);

    let hours = parse_relative_recovery("try again in 3 hours").expect("hours");
    assert!((hours - Local::now().naive_local()).num_hours() >= 2);
}

#[test]
fn hyphenated_window_is_used_when_no_reset_time_is_given() {
    let parsed = parse_relative_recovery("your token-plan 5-hour quota has been exhausted").expect("must parse");
    let delta = parsed - Local::now().naive_local();
    assert!(delta.num_hours() >= 4 && delta.num_hours() <= 5, "got {delta:?}");
}

#[test]
fn messages_without_a_reset_hint_return_none() {
    assert!(parse_relative_recovery("quota exhausted").is_none());
}

#[test]
fn oz_quota_limit_reached_is_recognized() {
    // Verbatim from `oz agent run`, exit code 1. Note it matches none of the
    // other four providers' wordings — "quota exceeded", "usage limit",
    // "quota has been exhausted" and "individual quota reached" all miss it.
    let (agent, minutes) = match_quota_signature("Error: Quota limit reached.")
        .expect("oz quota message must match");
    assert_eq!(agent, AgentKind::Oz);
    assert_eq!(minutes, 60);
}

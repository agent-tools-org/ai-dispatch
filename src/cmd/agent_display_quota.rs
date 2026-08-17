// Quota row + live-probe DETAIL for `aid agent quota` / list.
// Stale snapshots are tagged STALE and are never treated as Held.

use crate::live_quota;
use crate::rate_limit::{self, RateLimitInfo};
use crate::route_availability::{ProbeEvidence, QuotaWall};
use crate::types::AgentKind;
use std::time::Duration;

/// Observed quota state for display. A group hold is Partial, never Limited.
#[derive(Debug, PartialEq)]
pub(super) enum QuotaRow {
    Ok { detail: String },
    Limited { detail: String },
    Partial { detail: String },
}

pub(super) fn quota_row(kind: AgentKind, custom_name: Option<&str>) -> QuotaRow {
    if rate_limit::is_rate_limited(&kind, custom_name) {
        let info = rate_limit::get_rate_limit_info(&kind, custom_name);
        let hold = agent_hold_detail(&kind, custom_name, info.as_ref());
        return QuotaRow::Limited {
            detail: with_probe_suffix(kind, custom_name, hold, true),
        };
    }
    let groups = rate_limit::active_group_holds(&kind, custom_name);
    if groups.is_empty() {
        return QuotaRow::Ok {
            detail: probe_detail(kind).unwrap_or_else(|| "(no probe)".to_string()),
        };
    }
    QuotaRow::Partial {
        detail: with_probe_suffix(
            kind,
            custom_name,
            group_holds_detail(&kind, custom_name, &groups),
            false,
        ),
    }
}

fn with_probe_suffix(
    kind: AgentKind,
    custom_name: Option<&str>,
    detail: String,
    include_wall: bool,
) -> String {
    let Some(probe) = probe_detail(kind) else {
        return detail;
    };
    if include_wall {
        let wall = crate::route_availability::availability(&kind, custom_name).wall;
        if !matches!(wall, QuotaWall::None) {
            return format!("{detail}  ({} {probe})", wall_label(wall));
        }
    }
    format!("{detail}  {probe}")
}

fn wall_label(wall: QuotaWall) -> &'static str {
    match wall {
        QuotaWall::Clock => "clock",
        QuotaWall::Windowed => "windowed",
        QuotaWall::Prepaid => "prepaid",
        QuotaWall::PlanChange => "plan-change",
        QuotaWall::Transient => "transient",
        QuotaWall::None => "none",
    }
}

fn probe_detail(kind: AgentKind) -> Option<String> {
    let probe = live_quota::snapshot(&kind)?;
    probe.ok.then(|| format_probe(&probe))
}

fn format_probe(probe: &ProbeEvidence) -> String {
    let window = probe
        .windows
        .iter()
        .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent));
    let mut parts = Vec::new();
    if let Some(window) = window {
        parts.push(format!("{:.0}%", window.used_percent));
        if !window.label.is_empty() {
            parts.push(window.label.clone());
        }
        if let Some(at) = window.resets_at {
            parts.push(format!("resets {}", at.format("%Y-%m-%dT%H:%MZ")));
        }
    }
    parts.push(format!("probe {}", format_age(probe.age)));
    if probe.stale {
        parts.push("STALE".to_string());
    }
    parts.join("  ")
}

fn format_age(age: Duration) -> String {
    let secs = age.as_secs();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}

fn agent_hold_detail(
    kind: &AgentKind,
    custom_name: Option<&str>,
    info: Option<&RateLimitInfo>,
) -> String {
    let Some(info) = info else {
        return String::new();
    };
    let end = rate_limit::format_hold_end(kind, custom_name, info);
    match info.message.as_deref() {
        Some(msg) if !msg.is_empty() => format!("{end} — {msg}"),
        _ => end,
    }
}

fn group_holds_detail(
    kind: &AgentKind,
    custom_name: Option<&str>,
    groups: &[(String, RateLimitInfo)],
) -> String {
    groups
        .iter()
        .map(|(group, info)| {
            let end = rate_limit::format_hold_end(kind, custom_name, info);
            match info.message.as_deref() {
                Some(msg) if !msg.is_empty() => format!("{group} {end} — {msg}"),
                _ => format!("{group} {end}"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    use crate::live_quota::CacheDirGuard;
    use crate::paths::{self, AidHomeGuard};
    use crate::rate_limit::is_rate_limited;
    use chrono::{Duration as ChronoDuration, Utc};

    fn isolated() -> (tempfile::TempDir, AidHomeGuard, CacheDirGuard) {
        let temp = tempfile::tempdir().expect("tempdir");
        let _ = std::fs::create_dir_all(temp.path().join(".aid"));
        let home = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(paths::aid_dir()).ok();
        let aidbar = temp.path().join("aidbar");
        std::fs::create_dir_all(&aidbar).expect("cache");
        let cache = CacheDirGuard::set(&aidbar);
        (temp, home, cache)
    }

    fn write_grok(cache: &std::path::Path, used: f64, minutes_ago: i64) {
        let fetched = Utc::now() - ChronoDuration::minutes(minutes_ago);
        std::fs::write(
            cache.join("grok.json"),
            format!(
                r#"{{"ok":true,"snapshot":{{"provider":"grok","windows":[{{"label":"Aug 11 – Aug 18","used_percent":{used},"resets_at":"2026-08-18T00:55:28Z"}}],"fetched_at":"{}"}}}}"#,
                fetched.to_rfc3339()
            ),
        )
        .expect("cache");
    }

    #[test]
    fn agent_display_shows_live_percent_and_freshness() {
        let (temp, _home, _cache) = isolated();
        write_grok(&temp.path().join("aidbar"), 12.0, 3);
        match quota_row(AgentKind::Grok, None) {
            QuotaRow::Ok { detail } => {
                assert!(detail.contains("12%"), "{detail}");
                assert!(detail.contains("Aug 11"), "{detail}");
                assert!(detail.contains("probe 3m ago"), "{detail}");
                assert!(!detail.contains("STALE"), "{detail}");
            }
            other => panic!("expected Ok, got {other:?}"),
        }
        assert!(!is_rate_limited(&AgentKind::Grok, None));
    }

    #[test]
    fn agent_display_tags_stale_probe_and_does_not_hold() {
        let (temp, _home, _cache) = isolated();
        write_grok(&temp.path().join("aidbar"), 0.0, 20);
        match quota_row(AgentKind::Grok, None) {
            QuotaRow::Ok { detail } => {
                assert!(detail.contains("0%"), "{detail}");
                assert!(detail.contains("STALE"), "{detail}");
            }
            other => panic!("stale must stay Ok, got {other:?}"),
        }
        assert!(
            !is_rate_limited(&AgentKind::Grok, None),
            "stale is ranking/display only"
        );
    }
}

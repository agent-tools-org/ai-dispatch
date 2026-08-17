// Best-effort aidbar refresh for advise/quota. Dispatch never calls this.
// Exports: refresh_stale_if_enabled. Per-id spawn only; no whole-set refresh.
// Deps: live_quota snapshots, PATH probe.

use crate::live_quota;
use crate::types::AgentKind;

const ENV_REFRESH: &str = "AID_QUOTA_REFRESH";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RefreshDecision {
    Disabled,
    StayOnDisk { reason: &'static str },
    Refresh { providers: Vec<&'static str> },
}

/// aidbar has `--no-cache` (whole set, sequential; grok HTTP timeout is 10s)
/// and no per-id flag yet. Stay on disk until that flag exists.
fn per_id_refresh_supported() -> bool {
    false
}

const MAPPED: &[AgentKind] = &[
    AgentKind::Codex,
    AgentKind::Claude,
    AgentKind::OpenCode,
    AgentKind::Cursor,
    AgentKind::Antigravity,
    AgentKind::Grok,
    AgentKind::Qwen,
];

pub(crate) fn refresh_allowed(env_value: Option<&str>) -> bool {
    env_value != Some("0")
}

pub(crate) fn decide_refresh(
    env_value: Option<&str>,
    aidbar_on_path: bool,
    stale_providers: &[&'static str],
    per_id_supported: bool,
) -> RefreshDecision {
    if !refresh_allowed(env_value) {
        return RefreshDecision::Disabled;
    }
    if !aidbar_on_path {
        return RefreshDecision::StayOnDisk {
            reason: "aidbar not on PATH",
        };
    }
    if stale_providers.is_empty() {
        return RefreshDecision::StayOnDisk { reason: "cache fresh" };
    }
    if !per_id_supported {
        return RefreshDecision::StayOnDisk {
            reason: "no per-id aidbar refresh",
        };
    }
    RefreshDecision::Refresh {
        providers: stale_providers.to_vec(),
    }
}

fn stale_mapped_providers() -> Vec<&'static str> {
    MAPPED
        .iter()
        .filter_map(|agent| {
            let name = live_quota::provider_name(agent)?;
            match live_quota::snapshot(agent) {
                Some(probe) if !probe.stale => None,
                _ => Some(name),
            }
        })
        .collect()
}

/// Advise / `aid agent quota` only. Failed or skipped spawn is absence of
/// evidence — stay on the disk cache and do not promise current percents.
pub(crate) fn refresh_stale_if_enabled() {
    let env = std::env::var(ENV_REFRESH).ok();
    let on_path = crate::agent::env::which_exists("aidbar");
    let stale = stale_mapped_providers();
    let decision = decide_refresh(
        env.as_deref(),
        on_path,
        &stale,
        per_id_refresh_supported(),
    );
    match decision {
        RefreshDecision::StayOnDisk { reason }
            if !stale.is_empty() && reason == "aidbar not on PATH" =>
        {
            aid_info!("[aid] quota refresh skipped: aidbar not on PATH");
        }
        // No per-id flag: do not run `aidbar --no-cache` for the whole set.
        RefreshDecision::Refresh { .. } => {}
        _ => {}
    }
}

#[cfg(test)]
#[path = "live_quota_refresh_tests.rs"]
mod tests;

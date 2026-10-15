// Billing states replace OK in live quota rows while preserving provider credit units.
// Isolates both AID_HOME and the live quota cache.

use super::*;
use crate::routing::quota::live::CacheDirGuard;
use crate::support::paths::AidHomeGuard;

#[test]
fn billing_states_replace_ok_in_quota_rows() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    for state in ["overage", "exhausted"] {
        std::fs::write(
            temp.path().join("codex.json"),
            serde_json::json!({
                "ok":true,"snapshot":{"provider":"codex","windows":[],
                    "fetched_at":"2099-01-01T00:00:00Z","billing":{"state":state,
                        "credits":{"balance":7.25,"unlimited":false}}}
            })
            .to_string(),
        )
        .expect("cache");
        match (state, quota_row(AgentKind::Codex, None)) {
            ("overage", QuotaRow::Overage { detail }) | ("exhausted", QuotaRow::Exhausted { detail }) => {
                assert!(detail.contains("credits balance 7.25"));
            }
            (_, other) => panic!("unexpected row {other:?}"),
        }
    }
}

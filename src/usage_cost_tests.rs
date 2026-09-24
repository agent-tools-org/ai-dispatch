// Unknown (NULL) task costs in agent analytics: counted apart, never shown as free.
// Deps: agent_analytics, render_agent_analytics, tests::make_task.

use super::tests::make_task;
use super::*;
use crate::types::AgentKind;
use crate::usage_report::render_agent_analytics;
use chrono::{Duration, Local};

#[test]
fn unknown_cost_is_reported_unknown_not_free() {
    let now = Local::now();
    let mut priced = make_task("t-priced", AgentKind::Codex, 1_000, 0.55);
    priced.created_at = now - Duration::hours(1);
    let mut unknown = make_task("t-unknown", AgentKind::Codex, 9_000, 0.0);
    unknown.cost_usd = None;
    unknown.created_at = now - Duration::hours(2);
    let window = UsageWindow::parse("7d").unwrap();
    let analytics = agent_analytics(&[unknown, priced], "codex", window, now);
    assert_eq!(analytics.stats.cost_usd, 0.55);
    assert_eq!(analytics.stats.unknown_cost_tasks, 1);
    assert_eq!(analytics.top_tasks[0].cost_usd, Some(0.55), "known costs rank first");
    assert_eq!(analytics.top_tasks[1].cost_usd, None);
    let rendered = render_agent_analytics(&analytics);
    assert!(rendered.contains("Cost: $0.55 + 1 unknown"), "{rendered}");
    assert!(rendered.contains("t-unknown | unknown |"), "{rendered}");
}

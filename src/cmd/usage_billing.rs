// Overage usage grouped from dispatch events, independent of today's billing cache.
// Applies task/session/team inputs and the same period boundaries as usage totals.

use crate::cost;
use crate::cost::usage::{UsageReport, UsageWindow};
use crate::store::Store;
use crate::types::Task;
use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Default, Debug, Serialize)]
pub(super) struct OverageUsage {
    pub agent: String,
    pub tasks: usize,
    pub estimated_cost_usd: f64,
    pub unknown_cost_tasks: usize,
}

pub(super) fn collect(
    store: &Store,
    tasks: &[Task],
    agent: Option<&str>,
    window: UsageWindow,
    now: DateTime<Local>,
) -> anyhow::Result<Vec<OverageUsage>> {
    let overage_tasks = store.overage_task_ids(window.range(now))?;
    if overage_tasks.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = BTreeMap::<String, OverageUsage>::new();
    for task in tasks {
        let name = task.agent_display_name();
        if agent.is_some_and(|filter| !name.eq_ignore_ascii_case(filter))
            || window
                .range(now)
                .is_some_and(|(start, end)| task.created_at < start || task.created_at > end)
        {
            continue;
        }
        if !overage_tasks.contains(task.id.as_str()) {
            continue;
        }
        let row = rows.entry(name.to_string()).or_insert_with(|| OverageUsage {
            agent: name.to_string(),
            ..Default::default()
        });
        row.tasks += 1;
        let estimate = task
            .cost_usd
            .filter(|cost| *cost > 0.0)
            .or_else(|| cost::estimate_overage_cost(task.tokens?, task.costing_model()?, task.agent));
        match estimate {
            Some(cost) => row.estimated_cost_usd += cost,
            None => row.unknown_cost_tasks += 1,
        }
    }
    Ok(rows.into_values().collect())
}

pub(super) fn render(rows: &[OverageUsage]) -> String {
    rows.iter()
        .map(|row| {
            format!(
                "{} overage: {} tasks | estimated cost {}\n",
                row.agent,
                row.tasks,
                cost::format_cost_total(row.estimated_cost_usd, row.unknown_cost_tasks),
            )
        })
        .collect()
}

#[derive(Serialize)]
struct BillingReport<'a> {
    #[serde(flatten)]
    report: UsageReport,
    overage: &'a [OverageUsage],
}

pub(super) fn json(report: UsageReport, rows: &[OverageUsage]) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&BillingReport {
        report,
        overage: rows,
    })?)
}

#[cfg(test)]
#[path = "usage_billing_tests.rs"]
mod tests;

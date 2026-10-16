// UTC-calendar monthly spend and billing for named API routes.
// Uses persisted task costs and route-specific rate-limit markers, never subscription probes.
use super::quota::billing::{Billing, BillingState};
use crate::config::api_route::{ApiRoute, OverBudget};
use crate::store::Store;
use crate::types::Task;
use anyhow::Result;
use chrono::{DateTime, Datelike, Local, TimeZone, Utc};

pub(crate) struct ApiBudget {
    pub billing: Billing,
    pub month: String,
    pub resets_at: DateTime<Utc>,
    pub spent: f64,
    pub budget: f64,
    pub pace: f64,
    pub unpriced: usize,
}

impl ApiBudget {
    pub(crate) fn detail(&self) -> String {
        let total = format!("${:.2} / ${} this month", self.spent, self.budget);
        if self.unpriced == 0 {
            total
        } else {
            format!("{total} ({} tasks unpriced)", self.unpriced)
        }
    }

    pub(crate) fn label(&self) -> String {
        format!("{}  {}", self.billing.label(), self.detail())
    }
}

pub(crate) fn month_bounds(now: DateTime<Utc>) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    let midnight = |year, month| {
        Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .earliest()
            .ok_or_else(|| anyhow::anyhow!("UTC calendar month has no midnight"))
    };
    Ok((midnight(now.year(), now.month())?, midnight(year, month)?))
}

pub(crate) fn calculate(route: &ApiRoute, tasks: &[Task], now: DateTime<Utc>) -> Result<ApiBudget> {
    let (start, end) = month_bounds(now)?;
    let mut spent = 0.0;
    let mut unpriced = 0;
    for task in tasks.iter().filter(|task| {
        task.agent_display_name() == route.id
            && task.status.is_terminal()
            && task
                .completed_at
                .is_some_and(|at| at >= start && at < end && at <= now)
    }) {
        match task
            .cost_usd
            .or_else(|| crate::cost::api_route::estimate(task.tokens, task.costing_model()))
        {
            Some(cost) => spent += cost,
            None => unpriced += 1,
        }
    }
    let elapsed = (now - start).num_seconds() as f64 / (end - start).num_seconds() as f64;
    let state = if spent < route.monthly_budget_usd {
        BillingState::InPlan
    } else if route.over_budget == OverBudget::Bill {
        BillingState::Overage
    } else {
        BillingState::Exhausted
    };
    Ok(ApiBudget {
        billing: Billing { state, credits: None },
        month: start.format("%Y-%m").to_string(),
        resets_at: end,
        spent,
        budget: route.monthly_budget_usd,
        unpriced,
        pace: if spent > route.monthly_budget_usd * elapsed * 1.25 {
            -2.0
        } else if end - now < chrono::Duration::days(7) && spent < route.monthly_budget_usd * elapsed * 0.8 {
            1.0
        } else {
            0.0
        },
    })
}

pub(crate) fn for_route(route: &ApiRoute, store: Option<&Store>) -> Result<ApiBudget> {
    let owned;
    let store = match store {
        Some(store) => Some(store),
        None => {
            owned = Store::open_read_only(&crate::support::paths::db_path())?;
            owned.as_ref()
        }
    };
    let now = Utc::now();
    let (start, end) = month_bounds(now)?;
    let tasks = store
        .map(|s| s.api_route_month_tasks(&route.id, start, end))
        .transpose()?
        .unwrap_or_default();
    calculate(route, &tasks, now)
}

pub(crate) fn hold(id: &str, reason: &str, next_month: bool) -> Result<()> {
    let path = super::rate_limit::marker_path(&crate::types::AgentKind::Custom, Some(id));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let end = if next_month {
        let (_, end) = month_bounds(Utc::now())?;
        format!(
            "recovery_at: {}\n",
            super::rate_limit::format_recovery(end.with_timezone(&Local).naive_local())
        )
    } else {
        "hold: manual\n".to_string()
    };
    std::fs::write(path, format!("{end}message: {reason}\n"))?;
    Ok(())
}

#[cfg(test)]
#[path = "api_budget_tests.rs"]
mod tests;

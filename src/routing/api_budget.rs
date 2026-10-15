// Local-calendar monthly spend and billing for named API routes.
// Uses persisted task costs and route-specific rate-limit markers, never subscription probes.
use super::quota::billing::{Billing, BillingState};
use crate::config::api_route::{ApiRoute, OverBudget};
use crate::store::Store;
use crate::types::{Task, TaskFilter};
use anyhow::Result;
use chrono::{DateTime, Datelike, Local, TimeZone};

pub(crate) struct ApiBudget {
    pub billing: Billing,
    pub spent: f64,
    pub budget: f64,
    pub pace: f64,
}

impl ApiBudget {
    pub(crate) fn detail(&self) -> String {
        format!("${:.2} / ${} this month", self.spent, self.budget)
    }

    pub(crate) fn label(&self) -> String {
        format!("{}  {}", self.billing.label(), self.detail())
    }
}

pub(crate) fn month_bounds(now: DateTime<Local>) -> Result<(DateTime<Local>, DateTime<Local>)> {
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    let midnight = |year, month| {
        Local
            .with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .earliest()
            .ok_or_else(|| anyhow::anyhow!("local calendar month has no midnight"))
    };
    Ok((midnight(now.year(), now.month())?, midnight(year, month)?))
}

pub(crate) fn calculate(route: &ApiRoute, tasks: &[Task], now: DateTime<Local>) -> Result<ApiBudget> {
    let (start, end) = month_bounds(now)?;
    let spent = tasks
        .iter()
        .filter(|task| {
            task.agent_display_name() == route.id
                && task.created_at >= start
                && task.created_at <= now
                && task.created_at < end
        })
        .filter_map(|task| task.cost_usd)
        .fold(0.0, |spent, cost| spent + cost);
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
        spent,
        budget: route.monthly_budget_usd,
        pace: if spent > route.monthly_budget_usd * elapsed * 1.25 {
            -2.0
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
    let tasks = store
        .map(|s| s.list_tasks(TaskFilter::All))
        .transpose()?
        .unwrap_or_default();
    calculate(route, &tasks, Local::now())
}

pub(crate) fn blocking_reason(id: &str) -> Option<String> {
    let route = match crate::config::api_route::find(id) {
        Ok(Some(route)) => route,
        Ok(None) => return None,
        Err(error) => return Some(format!("API route configuration error: {error:#}")),
    };
    match for_route(&route, None) {
        Ok(budget) if budget.billing.state == BillingState::Exhausted => Some(budget.label()),
        Ok(_) => None,
        Err(error) => Some(format!("route {id}: cannot read monthly spend: {error:#}")),
    }
}

pub(crate) fn hold(id: &str, reason: &str, next_month: bool) -> Result<()> {
    let path = super::rate_limit::marker_path(&crate::types::AgentKind::Custom, Some(id));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let end = if next_month {
        let (_, end) = month_bounds(Local::now())?;
        format!(
            "recovery_at: {}\n",
            super::rate_limit::format_recovery(end.naive_local())
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

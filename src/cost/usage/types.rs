// Usage report payloads, windows and per-agent rows.
// Shared by usage collection and rendering.
use super::start_of_day;
use anyhow::Result;
use chrono::{DateTime, Duration, Local};
use serde::Serialize;

pub struct BudgetStatus {
    pub over_limit: bool,
    pub near_limit: bool,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct UsageSnapshot {
    pub(crate) generated_at: DateTime<Local>,
    pub(crate) agent_rows: Vec<AgentUsageRow>,
    pub(crate) budget_rows: Vec<BudgetUsageRow>,
}

#[derive(Default, Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum UsageWindow {
    #[default]
    All,
    Today,
    Days(u32),
}

impl UsageWindow {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "all" => Ok(Self::All),
            "today" => Ok(Self::Today),
            "7d" => Ok(Self::Days(7)),
            "30d" => Ok(Self::Days(30)),
            other => anyhow::bail!("Unknown period '{}'", other),
        }
    }

    pub fn range(&self, now: DateTime<Local>) -> Option<(DateTime<Local>, DateTime<Local>)> {
        match self {
            Self::All => None,
            Self::Today => Some((start_of_day(now), now)),
            Self::Days(days) => {
                let duration = Duration::days(*days as i64);
                Some((now - duration, now))
            }
        }
    }

    pub fn previous_range(&self, now: DateTime<Local>) -> Option<(DateTime<Local>, DateTime<Local>)> {
        if let Some(duration) = self.duration() {
            let (start, _) = self.range(now)?;
            Some((start - duration, start))
        } else {
            None
        }
    }

    fn duration(&self) -> Option<Duration> {
        match self {
            Self::All => None,
            Self::Today => Some(Duration::days(1)),
            Self::Days(days) => Some(Duration::days(*days as i64)),
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::All => "all time",
            Self::Today => "today",
            Self::Days(7) => "last 7 days",
            Self::Days(30) => "last 30 days",
            Self::Days(_) => "custom window",
        }
    }

    pub fn previous_label(&self) -> String {
        format!("previous {}", self.description())
    }
}

#[derive(Serialize)]
pub struct AgentAnalytics {
    pub agent_name: String,
    pub window: UsageWindow,
    pub stats: AgentPeriodStats,
    pub cost_per_success: Option<f64>,
    pub trend: Option<AgentTrend>,
    pub top_tasks: Vec<TaskSummary>,
}

#[derive(Serialize)]
pub struct AgentPeriodStats {
    pub tasks: usize,
    pub success_count: usize,
    pub fail_count: usize,
    pub retry_count: usize,
    pub tokens: i64,
    /// Sum of known costs; `unknown_cost_tasks` ran with a NULL cost.
    pub cost_usd: f64,
    pub unknown_cost_tasks: usize,
    pub avg_duration_secs: f64,
}

#[derive(Serialize)]
pub struct AgentTrend {
    pub label: String,
    pub current: AgentTrendStats,
    pub previous: AgentTrendStats,
}

#[derive(Serialize)]
pub struct AgentTrendStats {
    pub tasks: usize,
    pub cost_usd: f64,
}

#[derive(Serialize)]
pub struct TaskSummary {
    pub id: String,
    pub prompt_snippet: String,
    /// `None` when the task's price is unknown.
    pub cost_usd: Option<f64>,
    pub duration_secs: Option<f64>,
}

#[derive(Serialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum UsageReport {
    Summary {
        window: UsageWindow,
        snapshot: UsageSnapshot,
    },
    Agent {
        analytics: AgentAnalytics,
    },
}

#[derive(Serialize)]
pub(crate) struct AgentUsageRow {
    pub(crate) name: String,
    pub(crate) tasks: usize,
    pub(crate) tokens: i64,
    pub(crate) cost_usd: f64,
    pub(crate) unknown_cost_tasks: usize,
    pub(crate) success_rate: f64,
    pub(crate) avg_duration_secs: f64,
    pub(crate) retry_count: usize,
    pub(crate) last_task_at: Option<DateTime<Local>>,
}

#[derive(Serialize)]
pub(crate) struct BudgetUsageRow {
    pub(crate) name: String,
    pub(crate) plan: Option<String>,
    pub(crate) window: Option<String>,
    pub(crate) tasks: u32,
    pub(crate) task_limit: Option<u32>,
    pub(crate) tokens: i64,
    pub(crate) token_limit: Option<i64>,
    pub(crate) cost_usd: f64,
    pub(crate) unknown_cost_tasks: usize,
    pub(crate) cost_limit_usd: Option<f64>,
    pub(crate) resets_at: Option<String>,
    pub(crate) notes: Option<String>,
}

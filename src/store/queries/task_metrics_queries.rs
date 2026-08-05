// Task metrics query methods for budget and agent health summaries.
// Exports Store budget usage, success-rate, and cost aggregate queries.
// Deps: Store, rusqlite, chrono, and AgentKind.

use anyhow::Result;
use chrono::{DateTime, Local};
use rusqlite::params;

use super::super::Store;
use crate::types::AgentKind;

impl Store {
    pub fn budget_usage_summary(
        &self,
        agent: &str,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        self.budget_usage_summary_for_agent(Some(agent), since)
    }

    pub fn budget_usage_summary_all(
        &self,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        self.budget_usage_summary_for_agent(None, since)
    }

    /// Summarize usage for a project budget by matching the persisted repo_path basename.
    /// Tasks do not store project ids, so this mirrors project display fallback identity.
    pub fn budget_usage_summary_for_project(
        &self,
        project_name: &str,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        let conn = self.db();
        let (task_count, total_tokens, total_cost): (i64, i64, f64) = conn.query_row(
            "SELECT COUNT(*) as task_count,
                    COALESCE(SUM(tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost
             FROM tasks
             WHERE (
                repo_path = ?1
                OR (
                    repo_path IS NOT NULL
                    AND length(repo_path) > length(?1)
                    AND substr(repo_path, length(repo_path) - length(?1) + 1) = ?1
                    AND substr(repo_path, length(repo_path) - length(?1), 1) = '/'
                )
             ) AND (?2 IS NULL OR created_at >= ?2)",
            params![project_name, since.map(|value| value.to_rfc3339())],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((u32::try_from(task_count)?, total_tokens, total_cost))
    }

    fn budget_usage_summary_for_agent(
        &self,
        agent: Option<&str>,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        let conn = self.db();
        let (task_count, total_tokens, total_cost): (i64, i64, f64) = conn.query_row(
            "SELECT COUNT(*) as task_count,
                    COALESCE(SUM(tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost
             FROM tasks WHERE (?1 IS NULL OR agent = ?1) AND (?2 IS NULL OR created_at >= ?2)",
            params![agent, since.map(|value| value.to_rfc3339())],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((u32::try_from(task_count)?, total_tokens, total_cost))
    }

    pub fn agent_success_rates(&self) -> Result<Vec<(AgentKind, f64, usize)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent,
                    SUM(CASE WHEN status IN ('done', 'merged') THEN 1 ELSE 0 END) as successes,
                    COUNT(*) as total
             FROM tasks
             WHERE status IN ('done', 'merged', 'failed')
             GROUP BY agent
             HAVING total >= 5",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let successes: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let rate = successes as f64 / total as f64;
            Ok((agent, rate, total as usize))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn agent_success_rates_by_category(&self, category: &str) -> Result<Vec<(AgentKind, f64, usize)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent,
                    SUM(CASE WHEN status IN ('done', 'merged') THEN 1 ELSE 0 END) as successes,
                    COUNT(*) as total
             FROM tasks
             WHERE status IN ('done', 'merged', 'failed') AND category = ?1
             GROUP BY agent
             HAVING total >= 5",
        )?;
        let rows = stmt.query_map(params![category], |row| {
            let agent_str: String = row.get(0)?;
            let successes: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let rate = successes as f64 / total as f64;
            Ok((agent, rate, total as usize))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn agent_avg_costs(&self) -> Result<Vec<(AgentKind, f64)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, AVG(cost_usd) as avg_cost
             FROM tasks
             WHERE cost_usd IS NOT NULL AND cost_usd > 0
             GROUP BY agent
             HAVING COUNT(*) >= 3",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let avg_cost: f64 = row.get(1)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            Ok((agent, avg_cost))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn agent_avg_durations(&self) -> Result<Vec<(AgentKind, i64)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, AVG(duration_ms) / 1000.0 as avg_duration_secs
             FROM tasks
             WHERE duration_ms IS NOT NULL AND duration_ms > 0
             GROUP BY agent
             HAVING COUNT(*) >= 3",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let duration: f64 = row.get(1)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            Ok((agent, duration.round() as i64))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
}

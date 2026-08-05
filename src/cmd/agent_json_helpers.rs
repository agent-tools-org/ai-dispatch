use anyhow::Result;
use chrono::Local;
use std::collections::HashMap;

use crate::agent::custom::CustomAgentConfig;
use crate::agent::classifier::TaskCategory;
use crate::types::AgentKind;
use crate::store::Store;
use crate::cmd::agent_json_types::{HistoryJson, CategoryHistoryJson};

pub fn command_installed(command: &str) -> bool {
    let binary = command.split_whitespace().next().unwrap_or_default();
    if binary.is_empty() {
        return false;
    }
    std::process::Command::new("which")
        .arg(binary)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn get_agent_capabilities(kind: AgentKind, custom_config: Option<&CustomAgentConfig>) -> HashMap<String, i32> {
    let mut caps = HashMap::new();
    if let Some(config) = custom_config {
        caps.insert(TaskCategory::Research.label().to_string(), config.capabilities.research);
        caps.insert(TaskCategory::SimpleEdit.label().to_string(), config.capabilities.simple_edit);
        caps.insert(TaskCategory::ComplexImpl.label().to_string(), config.capabilities.complex_impl);
        caps.insert(TaskCategory::Frontend.label().to_string(), config.capabilities.frontend);
        caps.insert(TaskCategory::Debugging.label().to_string(), config.capabilities.debugging);
        caps.insert(TaskCategory::Testing.label().to_string(), config.capabilities.testing);
        caps.insert(TaskCategory::Refactoring.label().to_string(), config.capabilities.refactoring);
        caps.insert(TaskCategory::Documentation.label().to_string(), config.capabilities.documentation);
    } else {
        for category in &[
            TaskCategory::Research,
            TaskCategory::SimpleEdit,
            TaskCategory::ComplexImpl,
            TaskCategory::Frontend,
            TaskCategory::Debugging,
            TaskCategory::Testing,
            TaskCategory::Refactoring,
            TaskCategory::Documentation,
        ] {
            let score = crate::agent::selection::AGENT_CAPABILITIES.iter()
                .find(|(k, _)| *k == kind)
                .and_then(|(_, scores)| scores.iter().find(|(c, _)| *c == *category))
                .map(|(_, s)| *s)
                .unwrap_or(1);
            caps.insert(category.label().to_string(), score);
        }
    }
    caps
}

pub fn get_agent_history(
    store: &Store,
    agent_name: &str,
    is_custom: bool,
) -> Result<Option<HistoryJson>> {
    let conn = store.db();
    let limit_date = (Local::now() - chrono::Duration::days(30)).to_rfc3339();

    // Check if overall count >= 5
    let (total, successes_opt, avg_duration_secs, avg_cost_usd): (i64, Option<i64>, Option<f64>, Option<f64>) = conn.query_row(
        "SELECT
            COUNT(*) as total,
            SUM(CASE WHEN status IN ('done', 'merged') THEN 1 ELSE 0 END) as successes,
            AVG(duration_ms) / 1000.0 as avg_duration_secs,
            AVG(cost_usd) as avg_cost_usd
         FROM tasks
         WHERE
             status IN ('done', 'merged', 'failed')
             AND created_at >= ?1
             AND (
                 (?2 = 0 AND agent = ?3 AND agent != 'custom')
                 OR (?2 = 1 AND agent = 'custom' AND custom_agent_name = ?3)
             )",
        rusqlite::params![limit_date, if is_custom { 1 } else { 0 }, agent_name],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        },
    )?;
    let successes = successes_opt.unwrap_or(0);

    if total < 5 {
        return Ok(None);
    }

    // Query categories
    let mut stmt = conn.prepare(
        "SELECT
            category,
            COUNT(*) as total,
            SUM(CASE WHEN status IN ('done', 'merged') THEN 1 ELSE 0 END) as successes,
            AVG(duration_ms) / 1000.0 as avg_duration_secs
         FROM tasks
         WHERE
             status IN ('done', 'merged', 'failed')
             AND created_at >= ?1
             AND (
                 (?2 = 0 AND agent = ?3 AND agent != 'custom')
                 OR (?2 = 1 AND agent = 'custom' AND custom_agent_name = ?3)
             )
             AND category IS NOT NULL
         GROUP BY category
         HAVING total >= 5",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![limit_date, if is_custom { 1 } else { 0 }, agent_name],
        |row| {
            let category: String = row.get(0)?;
            let cat_total: i64 = row.get(1)?;
            let cat_successes_opt: Option<i64> = row.get(2)?;
            let cat_successes = cat_successes_opt.unwrap_or(0);
            let cat_avg_duration: Option<f64> = row.get(3)?;
            Ok((
                category,
                CategoryHistoryJson {
                    tasks: cat_total as u64,
                    success_rate: cat_successes as f64 / cat_total as f64,
                    avg_duration_secs: cat_avg_duration,
                },
            ))
        },
    )?;

    let mut by_category = HashMap::new();
    for r in rows {
        let (category, cat_hist) = r?;
        by_category.insert(category, cat_hist);
    }

    Ok(Some(HistoryJson {
        window_days: 30,
        tasks: total as u64,
        success_rate: successes as f64 / total as f64,
        avg_duration_secs,
        avg_cost_usd,
        by_category,
    }))
}

// Agent metadata and outcome-aware history helpers for JSON command output.
// Exports: capability lookup, command checks, and agent history aggregation.
// Deps: Store, agent configuration, and task outcome types.

use anyhow::{anyhow, Result};
use chrono::Local;
use std::collections::HashMap;

use crate::agent::custom::CustomAgentConfig;
use crate::agent::classifier::TaskCategory;
use crate::types::{
    verify_required, AgentKind, DeliveryAssessment, TaskOutcome, TaskStatus, VerifyStatus,
};
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

    let samples = history_samples(
        &conn,
        &limit_date,
        is_custom,
        agent_name,
    )?;
    let samples: Vec<_> = samples
        .into_iter()
        .filter(|sample| !sample.outcome.is_unverified())
        .collect();
    let total = samples.len();
    if total < 5 {
        return Ok(None);
    }
    let successes = samples.iter().filter(|sample| sample.outcome.is_success()).count();

    let mut categories: HashMap<String, (usize, usize, f64, usize)> = HashMap::new();
    for sample in &samples {
        let Some(category) = sample.category.as_deref() else {
            continue;
        };
        let entry = categories.entry(category.to_string()).or_default();
        entry.0 += 1;
        entry.1 += usize::from(sample.outcome.is_success());
        if let Some(duration_ms) = sample.duration_ms {
            entry.2 += duration_ms as f64;
            entry.3 += 1;
        }
    }
    let by_category = categories
        .into_iter()
        .filter(|(_, (total, _, _, _))| *total >= 5)
        .map(|(category, (total, successes, duration_ms, duration_count))| {
            (
                category,
                CategoryHistoryJson {
                    tasks: total as u64,
                    success_rate: successes as f64 / total as f64,
                    avg_duration_secs: (duration_count > 0)
                        .then(|| duration_ms / duration_count as f64 / 1000.0),
                },
            )
        })
        .collect();

    Ok(Some(HistoryJson {
        window_days: 30,
        tasks: total as u64,
        success_rate: successes as f64 / total as f64,
        avg_duration_secs: average_duration_secs(&samples),
        avg_cost_usd: average_cost(&samples),
        by_category,
    }))
}

struct HistorySample {
    category: Option<String>,
    duration_ms: Option<i64>,
    cost_usd: Option<f64>,
    outcome: TaskOutcome,
}

fn history_samples(
    conn: &rusqlite::Connection,
    limit_date: &str,
    is_custom: bool,
    agent_name: &str,
) -> Result<Vec<HistorySample>> {
    let mut stmt = conn.prepare(
        "SELECT category, status, verify_status, verify, duration_ms, cost_usd, delivery_assessment
         FROM tasks
         WHERE status IN ('done', 'merged', 'failed')
           AND created_at >= ?1
           AND ((?2 = 0 AND agent = ?3 AND agent != 'custom')
                OR (?2 = 1 AND agent = 'custom' AND custom_agent_name = ?3))",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![limit_date, if is_custom { 1 } else { 0 }, agent_name],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        },
    )?;
    rows.map(|row| {
        let (category, status, verify_status, verify, duration_ms, cost_usd, delivery) = row?;
        let status = TaskStatus::parse_str(&status)
            .ok_or_else(|| anyhow!("unknown task status in agent history: {status}"))?;
        let verify_status = VerifyStatus::parse_str(&verify_status)
            .ok_or_else(|| anyhow!("unknown verify status in agent history: {verify_status}"))?;
        let delivery = delivery
            .as_deref()
            .and_then(DeliveryAssessment::parse_str);
        Ok(HistorySample {
            category,
            duration_ms,
            cost_usd,
            outcome: TaskOutcome::derive(
                status,
                verify_status,
                verify_required(verify.as_deref()),
            )
            .with_delivery_assessment(delivery),
        })
    })
    .collect()
}

fn average_duration_secs(samples: &[HistorySample]) -> Option<f64> {
    let (total, count) = samples.iter().fold((0.0, 0usize), |(total, count), sample| {
        match sample.duration_ms {
            Some(duration_ms) => (total + duration_ms as f64, count + 1),
            None => (total, count),
        }
    });
    (count > 0).then(|| total / count as f64 / 1000.0)
}

fn average_cost(samples: &[HistorySample]) -> Option<f64> {
    let (total, count) = samples.iter().fold((0.0, 0usize), |(total, count), sample| {
        match sample.cost_usd {
            Some(cost_usd) => (total + cost_usd, count + 1),
            None => (total, count),
        }
    });
    (count > 0).then(|| total / count as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Result};
    use chrono::Local;
    use rusqlite::params;

    #[test]
    fn agent_history_excludes_unverified_rows_from_success_rate_and_sample() -> Result<()> {
        let store = Store::open_memory()?;
        let created_at = Local::now().to_rfc3339();
        for index in 0..5 {
            store.db().execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at, verify_status, verify, category)
                 VALUES (?1, 'codex', 'prompt', 'done', ?2, 'passed', 'cargo test', 'testing')",
                params![format!("passed-{index}"), created_at],
            )?;
        }
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, verify_status, verify, category)
             VALUES ('timeout', 'codex', 'prompt', 'done', ?1, 'timed_out', 'cargo test', 'testing')",
            params![created_at],
        )?;

        let history = get_agent_history(&store, "codex", false)?
            .ok_or_else(|| anyhow!("expected agent history"))?;
        let category = history
            .by_category
            .get("testing")
            .ok_or_else(|| anyhow!("expected testing category history"))?;

        assert_eq!(history.tasks, 5);
        assert_eq!(history.success_rate, 1.0);
        assert_eq!(category.tasks, 5);
        assert_eq!(category.success_rate, 1.0);
        Ok(())
    }

    /// The largest real correction: 366 rows in the live store are delivered
    /// with a failed verification, and the old SQL counted every one of them as
    /// a success because the status said `done`.
    #[test]
    fn agent_history_counts_a_failed_verification_as_a_failure() -> Result<()> {
        let store = Store::open_memory()?;
        let created_at = Local::now().to_rfc3339();
        for index in 0..4 {
            store.db().execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at, verify_status, verify, category)
                 VALUES (?1, 'codex', 'prompt', 'done', ?2, 'passed', 'cargo test', 'testing')",
                params![format!("passed-{index}"), created_at],
            )?;
        }
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, verify_status, verify, category)
             VALUES ('broken', 'codex', 'prompt', 'done', ?1, 'failed', 'cargo test', 'testing')",
            params![created_at],
        )?;

        let history = get_agent_history(&store, "codex", false)?
            .ok_or_else(|| anyhow!("expected agent history"))?;

        assert_eq!(history.tasks, 5, "a broken task stays in the denominator");
        assert!(
            (history.success_rate - 0.8).abs() < f64::EPSILON,
            "expected 4/5, got {}",
            history.success_rate
        );
        Ok(())
    }
}

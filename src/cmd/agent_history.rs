// Batched agent history aggregation for JSON command and web responses.
// Exports: get_agent_histories.
// Deps: Store task history rows and outcome/status types.

use anyhow::{anyhow, Result};
use chrono::Local;
use std::collections::{HashMap, HashSet};

use crate::cmd::agent_json_types::{CategoryHistoryJson, HistoryJson};
use crate::store::Store;
use crate::types::{
    verify_required, DeliveryAssessment, TaskOutcome, TaskStatus, VerifyStatus,
};

pub(crate) fn get_agent_histories(
    store: &Store,
    agent_names: &[&str],
) -> Result<HashMap<String, Option<HistoryJson>>> {
    let requested: HashSet<&str> = agent_names.iter().copied().collect();
    let limit_date = (Local::now() - chrono::Duration::days(30)).to_rfc3339();
    let mut samples_by_agent = load_history_samples(store, &requested, &limit_date)?;
    Ok(agent_names
        .iter()
        .map(|name| {
            let samples = samples_by_agent.remove(*name).unwrap_or_default();
            ((*name).to_string(), history_from_samples(samples))
        })
        .collect())
}

fn load_history_samples(
    store: &Store,
    requested: &HashSet<&str>,
    limit_date: &str,
) -> Result<HashMap<String, Vec<HistorySample>>> {
    let mut samples_by_agent: HashMap<String, Vec<HistorySample>> = HashMap::new();
    {
        let conn = store.db();
        let mut statement = conn.prepare(
            "SELECT agent, custom_agent_name, category, status, verify_status, verify,
                    duration_ms, cost_usd, delivery_assessment
             FROM tasks
             WHERE status IN ('done', 'merged', 'failed') AND created_at >= ?1",
        )?;
        let rows = statement.query_map([limit_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?;
        for row in rows {
            let (agent, custom_name, category, status, verify_status, verify, duration_ms, cost_usd, delivery) = row?;
            let name = if agent == "custom" {
                custom_name.unwrap_or_else(|| "custom".to_string())
            } else {
                agent
            };
            if !requested.contains(name.as_str()) {
                continue;
            }
            let status = TaskStatus::parse_str(&status)
                .ok_or_else(|| anyhow!("unknown task status in agent history: {status}"))?;
            let verify_status = VerifyStatus::parse_str(&verify_status)
                .ok_or_else(|| anyhow!("unknown verify status in agent history: {verify_status}"))?;
            let delivery = delivery.as_deref().and_then(DeliveryAssessment::parse_str);
            samples_by_agent.entry(name).or_default().push(HistorySample {
                category,
                duration_ms,
                cost_usd,
                outcome: TaskOutcome::derive(
                    status,
                    verify_status,
                    verify_required(verify.as_deref()),
                )
                .with_delivery_assessment(delivery),
            });
        }
    }
    Ok(samples_by_agent)
}

struct HistorySample {
    category: Option<String>,
    duration_ms: Option<i64>,
    cost_usd: Option<f64>,
    outcome: TaskOutcome,
}

fn history_from_samples(samples: Vec<HistorySample>) -> Option<HistoryJson> {
    let samples: Vec<_> = samples
        .into_iter()
        .filter(|sample| !sample.outcome.is_unverified())
        .collect();
    let total = samples.len();
    if total < 5 {
        return None;
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
        .filter(|(_, (count, _, _, _))| *count >= 5)
        .map(|(category, (count, successes, duration_ms, duration_count))| {
            (
                category,
                CategoryHistoryJson {
                    tasks: count as u64,
                    success_rate: successes as f64 / count as f64,
                    avg_duration_secs: (duration_count > 0)
                        .then(|| duration_ms / duration_count as f64 / 1000.0),
                },
            )
        })
        .collect();

    Some(HistoryJson {
        window_days: 30,
        tasks: total as u64,
        success_rate: successes as f64 / total as f64,
        avg_duration_secs: average_duration_secs(&samples),
        avg_cost_usd: average_cost(&samples),
        by_category,
    })
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

        let histories = get_agent_histories(&store, &["codex"])?;
        let history = histories
            .get("codex")
            .and_then(Option::as_ref)
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

        let history = get_agent_histories(&store, &["codex"])?
            .remove("codex")
            .flatten()
            .ok_or_else(|| anyhow!("expected agent history"))?;

        assert_eq!(history.tasks, 5);
        assert!((history.success_rate - 0.8).abs() < f64::EPSILON);
        Ok(())
    }
}

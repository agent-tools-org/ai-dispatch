// Stored execution facts for aid.execution/1.
// Reads policy and settlement independently of legacy Task row mapping.
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use super::Store;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ExecutionPolicy {
    pub name: String,
    pub version: u32,
    pub agent: String,
    pub model: String,
    pub accepted_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WorkerSettlement {
    pub state: String,
    pub outcome: Option<String>,
    pub pid: Option<u32>,
    pub settled_at: Option<String>,
    pub detail: Option<String>,
}

pub(crate) struct ExecutionFacts {
    pub revision: i64,
    pub policy: Option<ExecutionPolicy>,
    pub settlement: Option<WorkerSettlement>,
}

impl Store {
    pub(crate) fn execution_facts(&self, id: &str) -> Result<Option<ExecutionFacts>> {
        let row = self
            .db()
            .query_row(
                "SELECT revision, execution_policy, worker_settlement FROM tasks WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(revision, policy, settlement)| {
            Ok(ExecutionFacts {
                revision,
                policy: policy.map(|json| serde_json::from_str(&json)).transpose()?,
                settlement: settlement.map(|json| serde_json::from_str(&json)).transpose()?,
            })
        })
        .transpose()
    }

    pub(crate) fn observe_worker_state(
        &self,
        id: &str,
        state: &str,
        pid: Option<u32>,
        detail: Option<String>,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(state, "pending" | "running" | "unconfirmed"),
            "Invalid worker state"
        );
        let state = serde_json::to_string(&WorkerSettlement {
            state: state.into(),
            pid,
            outcome: None,
            settled_at: None,
            detail,
        })?;
        self.db().execute(
            "UPDATE tasks SET worker_settlement = ?2 WHERE id = ?1
             AND COALESCE(json_extract(worker_settlement, '$.state'), '') != 'settled'
             AND worker_settlement IS NOT ?2",
            params![id, state],
        )?;
        Ok(())
    }

    pub(crate) fn is_caller_owned(&self, id: &str) -> Result<bool> {
        Ok(self
            .execution_facts(id)?
            .is_some_and(|facts| facts.policy.is_some()))
    }

    pub(crate) fn begin_agent_launch(&self, id: &str) -> Result<Option<String>> {
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        let previous: Option<String> = tx.query_row("SELECT agent_process FROM tasks WHERE id = ?1", [id], |r| r.get(0))?;
        let changed = tx.execute(r#"UPDATE tasks SET agent_process = '{"state":"starting"}' WHERE id = ?1
            AND (execution_policy IS NULL OR (json_extract(agent_process, '$.state') = 'not_started'
              AND COALESCE(json_extract(agent_process, '$.launched'), 0) = 0))"#, [id])?;
        anyhow::ensure!(changed == 1, "Caller-owned task refuses a second agent launch");
        tx.commit()?;
        Ok(previous)
    }

    pub(crate) fn recover_agent_launch(&self, id: &str, previous: Option<String>, launched: bool) -> Result<()> {
        let previous = if launched && previous.as_deref().is_some_and(|json| {
            serde_json::from_str::<AgentProcess>(json).is_ok_and(|agent| agent.state == "not_started")
        }) { Some(r#"{"state":"not_started","launched":true}"#.to_string()) } else { previous };
        self.db().execute("UPDATE tasks SET agent_process = ?2 WHERE id = ?1
            AND json_extract(agent_process, '$.state') = 'starting'", params![id, previous])?;
        Ok(())
    }

    pub(crate) fn execution_process(&self, id: &str) -> Result<(Option<AgentProcess>, bool, bool)> {
        let (agent, spawned, stopped): (Option<String>, bool, bool) = self.db().query_row(
            "SELECT agent_process, worker_spawned, execution_stop FROM tasks WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((
            agent.map(|json| serde_json::from_str(&json)).transpose()?,
            spawned,
            stopped,
        ))
    }

    pub(crate) fn record_execution_stop(&self, id: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET execution_stop = 1 WHERE id = ?1 AND execution_stop = 0",
            [id],
        )?;
        Ok(())
    }

    pub(crate) fn mark_worker_spawned(&self, id: &str) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET worker_spawned = 1 WHERE id = ?1 AND worker_spawned = 0",
            [id],
        )?;
        Ok(())
    }

    pub(crate) fn persist_settlement(&self, id: &str, outcome: &str) -> Result<bool> {
        let settlement = WorkerSettlement {
            state: "settled".into(),
            outcome: Some(outcome.into()),
            pid: None,
            settled_at: Some(chrono::Utc::now().to_rfc3339()),
            detail: None,
        };
        Ok(self.db().execute(
            "UPDATE tasks SET worker_settlement = ?2 WHERE id = ?1
            AND COALESCE(json_extract(worker_settlement, '$.state'), '') != 'settled'",
            params![id, serde_json::to_string(&settlement)?],
        )? == 1)
    }

    #[cfg(test)]
    pub(crate) fn settle_worker(&self, id: &str, outcome: &str, _detail: Option<&str>) -> Result<()> {
        self.persist_settlement(id, outcome)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "execution_protocol_tests.rs"]
mod tests;

impl Store {
    pub(crate) fn execution_snapshot(&self, id: &str) -> Result<Option<serde_json::Value>> {
        let json = self
            .db()
            .query_row(
                "SELECT json_object(
               'protocol', 'aid.execution/1', 'task_id', id, 'revision', revision,
               'policy', json(execution_policy),
               'task', json_object('status', status,
                 'terminal', json(CASE WHEN status IN ('done','failed','stopped','merged','skipped')
                                  THEN 'true' ELSE 'false' END),
                 'agent', CASE WHEN agent = 'custom' THEN custom_agent_name ELSE agent END,
                 'requested_model', model, 'observed_model', observed_model,
                 'attribution_source', attribution_source, 'exit_code', exit_code,
                 'completed_at', completed_at),
               'worker', json(worker_settlement)) FROM tasks WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AgentProcess {
    pub state: String,
    pub pid: Option<u32>,
    pub pgid: Option<u32>,
    pub started: Option<String>,
}

pub(super) fn record_agent_on(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    let Some(spec) = crate::runtime::background::load_spec_if_exists(id)? else {
        return Ok(());
    };
    let Some(pid) = spec.agent_pid else {
        return Ok(());
    };
    let pgid = Some(pid);
    let agent = AgentProcess {
        state: "spawned".into(),
        pid: Some(pid),
        pgid,
        started: spec.agent_started,
    };
    conn.execute(
        "UPDATE tasks SET agent_process = ?2 WHERE id = ?1",
        params![id, serde_json::to_string(&agent)?],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "execution_launch_tests.rs"]
mod launch_tests;

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

    #[cfg(test)]
    pub(crate) fn settle_worker(&self, id: &str, outcome: &str, _detail: Option<&str>) -> Result<()> {
        use crate::runtime::execution_close::{close_execution, Closer};
        let spec = crate::runtime::background::BackgroundRunSpec {
            task_id: id.into(), ..Default::default()
        };
        close_execution(self, id, outcome, if outcome == "launch_failed" {
            Closer::LaunchFailed
        } else { Closer::Stop }, Some(&spec))?;
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

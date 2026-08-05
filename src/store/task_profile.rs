// Store reads and writes for nullable declared task-profile dimensions.
// Exports: Store::update_task_profile(), Store::get_task_profile().
// Deps: rusqlite and declared profile domain types.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::types::{
    TaskBudget, TaskDifficulty, TaskProfileDeclaration, TaskRigor, TaskUrgency,
};

impl Store {
    pub fn update_task_profile(
        &self,
        task_id: &str,
        profile: TaskProfileDeclaration,
    ) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET declared_difficulty = ?2, declared_budget = ?3,
             declared_urgency = ?4, declared_rigor = ?5 WHERE id = ?1",
            params![
                task_id,
                profile.difficulty.map(|value| value.label()),
                profile.budget.map(|value| value.label()),
                profile.urgency.map(|value| value.label()),
                profile.rigor.map(|value| value.label()),
            ],
        )?;
        Ok(())
    }

    pub fn get_task_profile(&self, task_id: &str) -> Result<TaskProfileDeclaration> {
        let values = self.db().query_row(
            "SELECT declared_difficulty, declared_budget, declared_urgency, declared_rigor
             FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((
                row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?,
            )),
        ).optional()?;
        let Some((difficulty, budget, urgency, rigor)) = values else {
            return Ok(TaskProfileDeclaration::default());
        };
        Ok(TaskProfileDeclaration {
            difficulty: difficulty.as_deref().and_then(TaskDifficulty::parse_str),
            budget: budget.as_deref().and_then(TaskBudget::parse_str),
            urgency: urgency.as_deref().and_then(TaskUrgency::parse_str),
            rigor: rigor.as_deref().and_then(TaskRigor::parse_str),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_dimensions_round_trip_through_store() {
        let store = Store::open_memory().expect("store");
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
             VALUES ('t-profile', 'codex', 'prompt', 'pending', '2026-08-05T00:00:00Z')",
            [],
        ).expect("insert task");
        let profile = TaskProfileDeclaration {
            difficulty: Some(TaskDifficulty::Complex),
            budget: Some(TaskBudget::Premium),
            urgency: Some(TaskUrgency::Urgent),
            rigor: Some(TaskRigor::Critical),
        };

        store.update_task_profile("t-profile", profile).expect("save profile");

        assert_eq!(store.get_task_profile("t-profile").expect("load profile"), profile);
    }
}

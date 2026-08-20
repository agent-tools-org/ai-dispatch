// Agent-focused Store queries for observed model reporting.
// Exports: latest_observed_models.
// Deps: Store, rusqlite, and task model columns.

use std::collections::HashMap;

use anyhow::Result;

use super::super::Store;

impl Store {
    pub fn latest_observed_models(&self) -> Result<HashMap<String, String>> {
        let conn = self.db();
        let mut statement = conn.prepare(
            "SELECT agent, custom_agent_name, observed_model FROM tasks
             WHERE observed_model IS NOT NULL AND TRIM(observed_model) <> ''
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut models = HashMap::new();
        for row in rows {
            let (agent, custom_name, model) = row?;
            let key = if agent == "custom" {
                custom_name.unwrap_or_else(|| "custom".to_string())
            } else {
                agent
            };
            models.entry(key).or_insert(model);
        }
        Ok(models)
    }
}

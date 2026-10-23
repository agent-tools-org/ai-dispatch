// Filtered task reads for board and usage reports, without resolved prompts.
// Exports TaskReadFilter and Store read/count methods; depends on existing task SQL and row mapping.

use super::*;
use chrono::DateTime;

#[derive(Default)]
pub(crate) struct TaskReadFilter<'a> {
    pub(crate) group: Option<&'a str>,
    pub(crate) project: Option<Option<&'a str>>,
    pub(crate) caller: Option<Option<&'a crate::runtime::session::CallerSession>>,
}

impl Store {
    pub(crate) fn read_board_tasks(
        &self,
        filter: TaskFilter,
        scope: &TaskReadFilter<'_>,
        limit: Option<usize>,
    ) -> Result<(Vec<Task>, usize)> {
        let (base, mut values) = board_query(filter, scope);
        let conn = self.db();
        let snapshot = conn.unchecked_transaction()?;
        let total = snapshot.query_row(
            &format!("SELECT COUNT(*) FROM ({base})"),
            rusqlite::params_from_iter(&values),
            |row| row.get(0),
        )?;
        let sql = limited_board_query(base, filter, &mut values, limit)?;
        let mut stmt = snapshot.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(&values), row_to_task)?;
        Ok((rows.map(|row| row?).collect::<Result<Vec<_>>>()?, total))
    }

    pub(crate) fn read_report_tasks(
        &self,
        group: Option<&str>,
        agent: Option<&str>,
        range: Option<(DateTime<Local>, DateTime<Local>)>,
    ) -> Result<Vec<Task>> {
        let scope = TaskReadFilter {
            group,
            ..Default::default()
        };
        let (mut sql, mut values) = board_query(TaskFilter::All, &scope);
        sql = sql.replacen("agent, prompt,", "agent, '' AS prompt,", 1);
        if let Some(agent) = agent {
            values.push(agent.to_string());
            let n = values.len();
            sql.push_str(&format!(
                " AND (agent = ?{n} COLLATE NOCASE OR custom_agent_name = ?{n} COLLATE NOCASE \
                 OR agent GLOB '*[^ -~]*' OR (lower(agent) = 'antigravity' AND ?{n} = 'agy' COLLATE NOCASE))"
            ));
        }
        if let Some((start, end)) = range {
            values.extend([start.to_rfc3339(), end.to_rfc3339()]);
            let n = values.len();
            // SQLite rounds to milliseconds; retain exact chrono boundaries below.
            sql.push_str(&format!(
                " AND (julianday(created_at) IS NULL OR julianday(created_at) \
                 BETWEEN julianday(?{}) - 0.00000002 AND julianday(?{n}) + 0.00000002)",
                n - 1
            ));
        }
        sql.push_str(" ORDER BY created_at DESC, rowid DESC");
        let conn = self.db();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(&values), row_to_task)?;
        let tasks = rows.map(|row| row?).collect::<Result<Vec<_>>>()?;
        Ok(tasks
            .into_iter()
            .filter(|task| {
                agent.is_none_or(|name| task.agent_display_name().eq_ignore_ascii_case(name))
                    && range.is_none_or(|(start, end)| task.created_at >= start && task.created_at <= end)
            })
            .collect())
    }
}

fn board_query(filter: TaskFilter, scope: &TaskReadFilter<'_>) -> (String, Vec<String>) {
    let (base, mut values) = query_sql::list_tasks_query(filter);
    let mut sql = base
        .trim_end_matches(" ORDER BY created_at DESC")
        .replace("prompt, resolved_prompt,", "prompt, NULL AS resolved_prompt,");
    if !sql.contains(" WHERE ") {
        sql.push_str(" WHERE 1 = 1");
    }
    for (column, value) in [
        ("workgroup_id", scope.group),
        ("project_id", scope.project.flatten()),
    ] {
        if let Some(value) = value {
            values.push(value.to_string());
            sql.push_str(&format!(" AND {column} = ?{}", values.len()));
        }
    }
    if scope.project == Some(None) {
        sql.push_str(" AND project_id IS NULL");
    }
    match scope.caller {
        Some(Some(caller)) => {
            values.extend([caller.kind.clone(), caller.session_id.clone()]);
            sql.push_str(&format!(
                " AND caller_kind = ?{} AND caller_session_id = ?{}",
                values.len() - 1,
                values.len()
            ));
        }
        Some(None) => sql.push_str(" AND 0 = 1"),
        None => {}
    }
    (sql, values)
}

fn limited_board_query(
    base: String,
    filter: TaskFilter,
    values: &mut Vec<String>,
    limit: Option<usize>,
) -> Result<String> {
    let order = task_order(filter);
    let Some(limit) = limit else {
        return Ok(format!("{base} ORDER BY {order}"));
    };
    values.push(i64::try_from(limit)?.to_string());
    let (projection, condition) = base
        .split_once("FROM tasks")
        .ok_or_else(|| anyhow::anyhow!("Task query has no FROM clause"))?;
    Ok(format!(
        "{projection}FROM tasks WHERE rowid IN (
             SELECT rowid FROM tasks{condition} ORDER BY {order} LIMIT ?{}
         ) ORDER BY {order}",
        values.len()
    ))
}

fn task_order(filter: TaskFilter) -> &'static str {
    match filter {
        TaskFilter::Running | TaskFilter::Active => "created_at DESC, status ASC, rowid ASC",
        TaskFilter::All | TaskFilter::Today => "created_at DESC, rowid DESC",
    }
}

#[cfg(test)]
#[path = "read_queries_tests.rs"]
mod tests;

// Task similarity query methods for prompt reuse and agent selection.
// Exports Store::find_similar_tasks with local keyword scoring.
// Deps: Store, rusqlite, and task/agent status types.

use anyhow::Result;

use super::super::Store;
use crate::types::{AgentKind, TaskStatus};

const SIMILAR_TASK_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "that", "this", "have", "your", "task", "code",
    "into", "using", "while", "when", "then", "which",
];

fn extract_similar_keywords(prompt: &str) -> Vec<String> {
    let mut candidates: Vec<(String, usize)> = prompt
        .split_whitespace()
        .filter_map(|word| {
            let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());
            if cleaned.len() < 4 {
                return None;
            }
            let lower = cleaned.to_lowercase();
            if SIMILAR_TASK_STOPWORDS.contains(&lower.as_str()) {
                return None;
            }
            Some((lower, cleaned.len()))
        })
        .collect();
    candidates.sort_unstable_by_key(|(_, len)| std::cmp::Reverse(*len));
    candidates.truncate(3);
    candidates.into_iter().map(|(word, _)| word).collect()
}

impl Store {
    pub fn find_similar_tasks(
        &self,
        prompt: &str,
        limit: usize,
    ) -> Result<Vec<(String, AgentKind, TaskStatus)>> {
        let keywords = extract_similar_keywords(prompt);
        if limit == 0 || keywords.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, status, prompt FROM tasks
             WHERE status IN ('done', 'failed', 'merged')
             ORDER BY created_at DESC
             LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let agent_str: String = row.get(1)?;
            let status_str: String = row.get(2)?;
            let task_prompt: String = row.get(3)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            let status = TaskStatus::parse_str(&status_str).unwrap_or(TaskStatus::Failed);
            Ok((id, agent, status, task_prompt))
        })?;
        let mut scored = Vec::new();
        for row in rows {
            let (id, agent, status, task_prompt) = row?;
            let lower_prompt = task_prompt.to_lowercase();
            let score: usize = keywords
                .iter()
                .map(|keyword| lower_prompt.matches(keyword).count())
                .sum();
            if score > 0 {
                scored.push((score, id, agent, status));
            }
        }
        scored.sort_unstable_by_key(|(score, _, _, _)| std::cmp::Reverse(*score));
        scored.truncate(limit);
        Ok(scored
            .into_iter()
            .map(|(_, id, agent, status)| (id, agent, status))
            .collect())
    }
}

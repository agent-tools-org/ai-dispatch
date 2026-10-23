// Statistics collection state and ordered aggregation phases.
// Deps: parent snapshot types, Store, task cost helpers.
use super::*;

pub(super) struct StatsRun<'a> {
    agents: BTreeMap<String, (AgentKind, usize, usize, usize, i64, usize, Option<f64>)>,
    failures: HashMap<String, (usize, BTreeMap<String, usize>)>,
    models: BTreeMap<String, (usize, Option<f64>, AgentKind)>,
    declared: BTreeMap<String, (usize, i64, usize, usize)>,
    day_counts: HashMap<String, usize>,
    hour_counts: [usize; 24],
    total_cost: Option<f64>,
    total_tokens: i64,
    total_tasks: usize,
    longest: Option<(&'a Task, i64)>,
    most_tokens: Option<(&'a Task, i64)>,
    highest_cost: Option<(&'a Task, f64)>,
}
impl<'a> StatsRun<'a> {
    pub(super) fn new() -> Self {
        Self {
            agents: BTreeMap::new(),
            failures: HashMap::new(),
            models: BTreeMap::new(),
            declared: BTreeMap::new(),
            day_counts: HashMap::new(),
            hour_counts: [0usize; 24],
            total_cost: None,
            total_tokens: 0,
            total_tasks: 0,
            longest: None,
            most_tokens: None,
            highest_cost: None,
        }
    }
    pub(super) fn aggregate(&mut self, store: &Store, tasks: &'a [Task]) -> Result<()> {
        for task in tasks {
            let cost_usd = task_cost(store, task)?;
            let outcome = self.record_usage(task, cost_usd);
            self.record_declared(store, task, outcome)?;
            self.record_activity(task, cost_usd);
            self.record_failure(store, task, outcome);
        }
        Ok(())
    }
    fn record_usage(&mut self, task: &'a Task, cost_usd: Option<f64>) -> TaskOutcome {
        let row = self
            .agents
            .entry(task.agent_display_name().to_string())
            .or_insert((task.agent, 0, 0, 0, 0, 0, None));
        row.1 += 1;
        let outcome = task.outcome();
        row.2 += usize::from(outcome.is_success());
        row.3 += usize::from(!matches!(
            outcome,
            TaskOutcome::InProgress | TaskOutcome::Stopped
        ));
        if let Some(ms) = task.duration_ms {
            row.4 += ms;
            row.5 += 1;
        }
        add_known_cost(&mut row.6, cost_usd);
        let model = task.attributed_model().unwrap_or("unknown").to_string();
        let model_row = self.models.entry(model).or_insert((0, None, task.agent));
        model_row.0 += 1;
        add_known_cost(&mut model_row.1, cost_usd);
        outcome
    }
    fn record_declared(
        &mut self,
        store: &Store,
        task: &'a Task,
        outcome: TaskOutcome,
    ) -> Result<()> {
        if let Some(difficulty) = store.get_task_profile(task.id.as_str())?.difficulty {
            let row = self
                .declared
                .entry(difficulty.label().to_string())
                .or_default();
            row.0 += 1;
            if let Some(duration) = task.duration_ms {
                row.1 += duration;
                row.2 += 1;
            }
            row.3 += usize::from(matches!(outcome, TaskOutcome::Broken | TaskOutcome::Failed));
        }
        Ok(())
    }
    fn record_activity(&mut self, task: &'a Task, cost_usd: Option<f64>) {
        *self
            .day_counts
            .entry(task.created_at.format("%a").to_string())
            .or_default() += 1;
        self.hour_counts[task.created_at.hour() as usize] += 1;
        add_known_cost(&mut self.total_cost, cost_usd);
        self.total_tokens += task.tokens.unwrap_or(0);
        self.total_tasks += 1;
        if let Some(ms) = task.duration_ms {
            if self.longest.as_ref().is_none_or(|(_, best)| ms > *best) {
                self.longest = Some((task, ms));
            }
        }
        let tokens = task.tokens.unwrap_or(0);
        if self
            .most_tokens
            .as_ref()
            .is_none_or(|(_, best)| tokens > *best)
        {
            self.most_tokens = Some((task, tokens));
        }
        if let Some(cost_usd) = cost_usd {
            if self
                .highest_cost
                .as_ref()
                .is_none_or(|(_, best)| cost_usd > *best)
            {
                self.highest_cost = Some((task, cost_usd));
            }
        }
    }
    fn record_failure(&mut self, store: &Store, task: &'a Task, outcome: TaskOutcome) {
        if matches!(outcome, TaskOutcome::Broken | TaskOutcome::Failed) {
            let label = classify_failure(
                store.latest_error(task.id.as_str()).as_deref(),
                task.exit_code,
            );
            let entry = self.failures.entry(label).or_insert((0, BTreeMap::new()));
            entry.0 += 1;
            *entry
                .1
                .entry(task.agent_display_name().to_string())
                .or_default() += 1;
        }
    }
    fn agent_rows(&mut self) -> Vec<AgentRow> {
        let mut agent_rows: Vec<_> = std::mem::take(&mut self.agents)
            .into_iter()
            .map(
                |(
                    agent,
                    (kind, tasks, success, success_base, duration_ms, duration_count, cost_usd),
                )| AgentRow {
                    agent,
                    tasks,
                    share_pct: usage_share(tasks, self.total_tasks),
                    success_rate: if success_base == 0 {
                        0.0
                    } else {
                        success as f64 * 100.0 / success_base as f64
                    },
                    avg_duration_ms: (duration_count > 0)
                        .then(|| duration_ms / duration_count as i64),
                    cost: cost::format_cost_label(cost_usd, kind),
                },
            )
            .collect();
        agent_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.agent.cmp(&b.agent)));
        agent_rows
    }
    fn failure_rows(&mut self) -> Vec<FailureRow> {
        let mut failure_rows: Vec<_> = std::mem::take(&mut self.failures)
            .into_iter()
            .map(|(label, (tasks, agents))| {
                let mut agents: Vec<_> = agents.into_iter().collect();
                agents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                FailureRow {
                    label,
                    tasks,
                    agents,
                }
            })
            .collect();
        failure_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.label.cmp(&b.label)));
        failure_rows.truncate(5);
        failure_rows
    }
    fn model_rows(&mut self) -> Vec<ModelRow> {
        let mut model_rows: Vec<_> = std::mem::take(&mut self.models)
            .into_iter()
            .map(|(model, (tasks, cost_usd, kind))| ModelRow {
                model,
                tasks,
                cost: cost::format_cost_label(cost_usd, kind),
            })
            .collect();
        model_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.model.cmp(&b.model)));
        model_rows
    }
    fn declared_rows(&mut self) -> Vec<DeclaredRow> {
        std::mem::take(&mut self.declared)
            .into_iter()
            .map(
                |(difficulty, (tasks, duration, count, failures))| DeclaredRow {
                    difficulty,
                    tasks,
                    avg_duration_ms: (count > 0).then(|| duration / count as i64),
                    failures,
                },
            )
            .collect()
    }
    pub(super) fn finish(mut self) -> Result<StatsSnapshot> {
        let agent_rows = self.agent_rows();
        let failure_rows = self.failure_rows();
        let model_rows = self.model_rows();
        let declared_rows = self.declared_rows();
        Ok(StatsSnapshot {
            agent_rows,
            failure_rows,
            model_rows,
            declared_rows,
            total_cost: self.total_cost,
            total_tokens: self.total_tokens,
            total_tasks: self.total_tasks,
            activity_by_day: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .into_iter()
                .map(|day| (day.to_string(), *self.day_counts.get(day).unwrap_or(&0)))
                .collect(),
            activity_by_hour: self
                .hour_counts
                .into_iter()
                .enumerate()
                .map(|(hour, count)| (hour as u32, count))
                .collect(),
            top_sessions: self.top_sessions(),
        })
    }
    fn top_sessions(&self) -> Vec<TopSession> {
        [
            self.longest.map(|(task, ms)| TopSession {
                task_id: task.id.to_string(),
                agent: task.display_route(),
                label: "Longest",
                value: format_duration(Some(ms)),
            }),
            self.most_tokens.map(|(task, tokens)| TopSession {
                task_id: task.id.to_string(),
                agent: task.display_route(),
                label: "Most tokens",
                value: format_tokens(tokens),
            }),
            self.highest_cost.map(|(task, cost_usd)| TopSession {
                task_id: task.id.to_string(),
                agent: task.display_route(),
                label: "Highest cost",
                value: cost::format_cost(Some(cost_usd)),
            }),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

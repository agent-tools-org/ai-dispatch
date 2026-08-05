// Task-centric domain structs for aid task storage and display.
// Exports: Task, Workgroup, Finding, TaskEvent, TaskFilter, CompletionInfo.
// Deps: chrono, serde, and parent `crate::types` enums/IDs.

use chrono::{DateTime, Local};
use serde::Serialize;

use super::{
    AgentKind, AttributionSource, DeliveryAssessment, EventKind, TaskId, TaskStatus, VerifyStatus,
    WorkgroupId,
};

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: TaskId,
    pub agent: AgentKind,
    pub custom_agent_name: Option<String>,
    pub prompt: String,
    pub resolved_prompt: Option<String>,
    pub category: Option<String>,
    pub status: TaskStatus,
    pub parent_task_id: Option<String>,
    pub workgroup_id: Option<String>,
    pub caller_kind: Option<String>,
    pub caller_session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub final_head_sha: Option<String>,
    pub final_branch: Option<String>,
    pub start_sha: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    /// The model aid dispatched with — a request, not an outcome. Set at
    /// dispatch from `--model`, the configured default, budget mode or smart
    /// routing, and kept as aid passed it even when the CLI refused to serve
    /// it: `t-bd455a68` asked the `claude` CLI for `gemini-3.6-flash-low` and
    /// failed, and the request is still the honest record of what was asked.
    pub requested_model: Option<String>,
    /// The model the CLI reported it actually ran. `None` means the CLI never
    /// said so — which is not the same as the requested model having run.
    /// Collapsing the two is what stored a cursor model on an `agy` task
    /// (`t-702f7bcb`) and `auto`, a router, as though it were a model.
    ///
    /// Cost, capability history and model-level routing must read this, never
    /// `requested_model`. Per-family quota marking is the one legitimate reader
    /// of the request: it asks which family aid *aimed at*, and plain-text CLIs
    /// such as agy never echo a model at all.
    pub observed_model: Option<String>,
    /// How `observed_model` was established. Always `None` when
    /// `observed_model` is `None` — the two move together.
    pub attribution_source: Option<AttributionSource>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub verify: Option<String>,
    pub verify_status: VerifyStatus,
    pub pending_reason: Option<String>,
    pub read_only: bool,
    pub budget: bool,
    pub audit_verdict: Option<String>,
    pub audit_report_path: Option<String>,
    pub delivery_assessment: Option<DeliveryAssessment>,
}

impl Task {
    /// The model an outcome may be attributed to: what the CLI reported, never
    /// what aid asked for. `None` means nobody knows, and it must stay unknown
    /// — capability history, per-model success rates and model-level routing
    /// read this, and a guessed model there poisons the advice built on it.
    pub fn attributed_model(&self) -> Option<&str> {
        self.observed_model.as_deref()
    }

    /// The model, but only when the CLI itself said so. Capability scoring and
    /// an agent's learned default model read this rather than
    /// `attributed_model`, because a model inferred from a run merely not
    /// failing is not evidence that model performed well — or even that a
    /// substitution did not happen behind a successful exit.
    pub fn conclusive_model(&self) -> Option<&str> {
        self.attribution_source
            .filter(|source| source.is_conclusive())
            .and(self.observed_model.as_deref())
    }

    /// The model to price against, and the model a derived dispatch should ask
    /// for again: the observation when there is one, otherwise the original
    /// request. The fallback is legitimate here only because both values are
    /// stored, so a reader can see which basis was used.
    pub fn costing_model(&self) -> Option<&str> {
        self.observed_model
            .as_deref()
            .or(self.requested_model.as_deref())
    }

    /// What to show a human. A request that was never confirmed is marked, and
    /// an observation that contradicts the request is shown as both — that
    /// disagreement means the CLI served something other than what was asked.
    pub fn display_model(&self) -> Option<String> {
        format_model_display(
            self.observed_model.as_deref(),
            self.requested_model.as_deref(),
            self.attribution_source,
        )
    }

    pub fn agent_display_name(&self) -> &str {
        if self.agent == AgentKind::Custom {
            self.custom_agent_name.as_deref().unwrap_or("custom")
        } else {
            self.agent.as_str()
        }
    }

    pub fn delivery_assessment(&self) -> Option<DeliveryAssessment> {
        self.delivery_assessment
    }

    pub fn has_verify_failure(&self) -> bool {
        self.verify_status == VerifyStatus::Failed
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Workgroup {
    pub id: WorkgroupId,
    pub name: String,
    pub shared_context: String,
    pub created_by: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: i64,
    pub workgroup_id: String,
    pub content: String,
    pub source_task_id: Option<String>,
    pub severity: Option<String>,
    pub title: Option<String>,
    pub file: Option<String>,
    pub lines: Option<String>,
    pub category: Option<String>,
    pub confidence: Option<String>,
    pub verdict: Option<String>,
    pub score: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskEvent {
    pub task_id: TaskId,
    pub timestamp: DateTime<Local>,
    pub event_kind: EventKind,
    pub detail: String,
    pub metadata: Option<serde_json::Value>,
}

impl TaskEvent {
    /// Untruncated detail: parsers stash text over the display cap in
    /// metadata under `"full"`.
    pub fn full_detail(&self) -> &str {
        self.metadata
            .as_ref()
            .and_then(|meta| meta.get("full"))
            .and_then(|value| value.as_str())
            .unwrap_or(&self.detail)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TaskFilter {
    All,
    Active,
    Running,
    Today,
}

#[derive(Debug, Clone)]
pub struct CompletionInfo {
    pub tokens: Option<i64>,
    pub status: TaskStatus,
    /// The model the CLI named in its own output, if it named one at all. An
    /// adapter must never put the dispatched request here.
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
}

/// Renders model attribution for humans. An unconfirmed request is marked with
/// `?` so a reader can tell a guess from an observation at a glance, and a
/// disagreement is shown in full because it means the CLI served something
/// other than what was asked for.
pub fn format_model_display(
    observed: Option<&str>,
    requested: Option<&str>,
    source: Option<AttributionSource>,
) -> Option<String> {
    match (observed, requested) {
        (Some(observed), Some(requested)) if observed != requested => {
            Some(format!("{observed} (asked {requested})"))
        }
        // Inferred from the run not failing rather than from the CLI saying so.
        // Rendering it identically to an echo would hide the weaker evidence,
        // which is the whole reason the grade is stored.
        (Some(observed), _) if source == Some(AttributionSource::ConfirmedBySuccess) => {
            Some(format!("{observed} (inferred)"))
        }
        (Some(observed), _) => Some(observed.to_string()),
        (None, Some(requested)) => Some(format!("{requested}?")),
        (None, None) => None,
    }
}

#[cfg(test)]
mod model_display_tests {
    use super::format_model_display;

    #[test]
    fn an_unconfirmed_request_is_marked_as_one() {
        assert_eq!(format_model_display(None, Some("gpt-5.6"), None), Some("gpt-5.6?".to_string()));
    }

    #[test]
    fn a_confirmed_model_is_shown_plainly() {
        assert_eq!(
            format_model_display(Some("gpt-5.6"), Some("gpt-5.6"), None),
            Some("gpt-5.6".to_string())
        );
    }

    /// The case worth surfacing: aid asked for one model and the CLI served
    /// another. Showing only one of the two hides a substitution.
    #[test]
    fn a_substitution_shows_both() {
        assert_eq!(
            format_model_display(Some("composer-2"), Some("auto"), None),
            Some("composer-2 (asked auto)".to_string())
        );
    }

    #[test]
    fn nothing_known_renders_as_nothing() {
        assert_eq!(format_model_display(None, None, None), None);
    }
}

#[cfg(test)]
mod attribution_grade_display_tests {
    use super::{format_model_display, AttributionSource};

    /// A model inferred from a run not failing must not read the same as one the
    /// CLI named. Storing the grade and then rendering both identically would
    /// waste it.
    #[test]
    fn an_inferred_model_is_marked() {
        assert_eq!(
            format_model_display(
                Some("gpt-5.6"),
                Some("gpt-5.6"),
                Some(AttributionSource::ConfirmedBySuccess)
            ),
            Some("gpt-5.6 (inferred)".to_string())
        );
    }

    #[test]
    fn an_echoed_model_stays_plain() {
        assert_eq!(
            format_model_display(Some("gpt-5.6"), Some("gpt-5.6"), Some(AttributionSource::Echoed)),
            Some("gpt-5.6".to_string())
        );
    }

    /// A disagreement outranks the grade: the CLI serving something other than
    /// what was asked is the more important thing to show.
    #[test]
    fn a_substitution_still_shows_both() {
        assert_eq!(
            format_model_display(Some("composer-2"), Some("auto"), Some(AttributionSource::Echoed)),
            Some("composer-2 (asked auto)".to_string())
        );
    }
}

// Event construction and display, including trimmed historical output.
// Extends TaskEvent; preserves full detail and structured evidence.
use super::{TaskEvent, TaskId, EventKind};
use chrono::Local;

impl TaskEvent {
    pub(crate) fn trimmed_note(&self) -> Option<&'static str> {
        let trimmed = |key| {
            self.metadata
                .as_ref()
                .and_then(|meta| meta.get(key))
                .is_some_and(|value| value.get("trimmed").and_then(|v| v.as_bool()) == Some(true))
        };
        match (trimmed("full"), trimmed("provider_stdout")) {
            (true, true) => Some("full and provider_stdout output trimmed"),
            (true, false) => Some("full output trimmed"),
            (false, true) => Some("provider_stdout output trimmed"),
            (false, false) => None,
        }
    }

    pub fn display_detail(&self) -> std::borrow::Cow<'_, str> {
        self.render_detail(&self.detail)
    }

    pub(super) fn render_detail<'a>(&self, detail: &'a str) -> std::borrow::Cow<'a, str> {
        match self.trimmed_note() {
            Some(note) => format!("{detail} [{note}]").into(),
            None => detail.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_rendering_preserves_retained_values_and_identifies_each_marker() {
        let marker = json!({"trimmed": true, "bytes": 5000});
        let mut event = TaskEvent::new(
            crate::types::TaskId("trimmed".into()),
            crate::types::EventKind::Build,
            "retained detail".into(),
        );
        for (metadata, expected) in [
            (
                json!({"full":"retained full","provider_stdout":marker}),
                "retained full [provider_stdout output trimmed]",
            ),
            (
                json!({"provider_stdout":marker}),
                "retained detail [provider_stdout output trimmed]",
            ),
            (
                json!({"full":marker,"provider_stdout":"raw envelope"}),
                "retained detail [full output trimmed]",
            ),
            (
                json!({"full":marker,"provider_stdout":marker}),
                "retained detail [full and provider_stdout output trimmed]",
            ),
        ] {
            event.metadata = Some(metadata);
            assert_eq!(event.full_detail(), expected);
            assert!(event.display_detail().starts_with("retained detail"));
        }
    }
}

impl TaskEvent {
    pub(crate) fn new(task_id: TaskId, event_kind: EventKind, detail: String) -> Self {
        Self::at(task_id, event_kind, detail, Local::now())
    }

    pub(crate) fn at(
        task_id: TaskId,
        event_kind: EventKind,
        detail: String,
        timestamp: chrono::DateTime<Local>,
    ) -> Self {
        Self {
            task_id,
            timestamp,
            event_kind,
            detail,
            metadata: None,
        }
    }

    pub(crate) fn with_metadata(mut self, metadata: Option<serde_json::Value>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Untruncated detail: parsers stash text over the display cap in
    /// metadata under `"full"`.
    pub fn full_detail(&self) -> std::borrow::Cow<'_, str> {
        let detail = self.metadata
            .as_ref()
            .and_then(|meta| meta.get("full"))
            .and_then(|value| value.as_str())
            .unwrap_or(&self.detail);
        self.render_detail(detail)
    }
}

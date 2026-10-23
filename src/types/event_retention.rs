// Display helpers for historical events whose bulky output has been trimmed.
// Extends TaskEvent; event detail and structured evidence remain available.
use super::TaskEvent;

impl TaskEvent {
    pub fn output_trimmed(&self) -> bool {
        self.metadata
            .as_ref()
            .and_then(|meta| meta.get("full"))
            .is_some_and(|value| value.get("trimmed").and_then(|v| v.as_bool()) == Some(true))
    }

    pub fn display_detail(&self) -> std::borrow::Cow<'_, str> {
        if self.output_trimmed() {
            format!("{} [output trimmed]", self.detail).into()
        } else {
            self.detail.as_str().into()
        }
    }
}

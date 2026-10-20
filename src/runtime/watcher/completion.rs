// Completion checkpoint application and watcher cost ceiling checks.
// Shared by streaming ingestion and its existing tests.
use super::*;

pub(super) fn apply_completion_event(info: &mut CompletionInfo, event: &TaskEvent) {
    let Some(metadata) = event.metadata.as_ref() else {
        return;
    };
    // A model announcement is evidence wherever it appears in the stream, not
    // only on a completion event. copilot names its model on a Milestone
    // (`session.tools_updated` -> `parse_model_update`) and droid names it on
    // its very first `system/init` line; neither repeats it at completion.
    // Gating the whole function on `EventKind::Completion` dropped both, which
    // is why copilot recorded 19 of 20 tasks with no model and droid 14 of 15,
    // while `"model":"gpt-5-mini"` sat in copilot's log three times over and
    // `"model":"claude-opus-5"` on line 0 of droid's.
    if let Some(model) = metadata.get("model").and_then(|value| value.as_str()) {
        info.model = Some(model.to_string());
    }
    // Tokens and cost stay completion-only: a mid-run value is partial, and
    // letting an interim total overwrite the final one would understate both.
    if event.event_kind != EventKind::Completion {
        return;
    }
    let incremental = metadata
        .get("usage_is_delta")
        .and_then(|value| value.as_bool())
        == Some(true);
    if let Some(tokens) = metadata.get("tokens").and_then(|value| value.as_i64()) {
        info.tokens = Some(if incremental {
            info.tokens.unwrap_or_default() + tokens
        } else {
            tokens
        });
    }
    if let Some(cost_usd) = metadata.get("cost_usd").and_then(|value| value.as_f64()) {
        info.cost_usd = Some(if incremental {
            info.cost_usd.unwrap_or_default() + cost_usd
        } else {
            cost_usd
        });
    }
}

pub(super) fn exceeds_cost_ceiling(current_cost: Option<f64>, max_task_cost: Option<f64>) -> bool {
    matches!(
        (current_cost, max_task_cost),
        (Some(current_cost), Some(max_task_cost)) if current_cost > max_task_cost
    )
}

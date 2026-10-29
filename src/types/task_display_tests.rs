// Tests for model and route display helpers on Task.
// Loaded by task.rs under `#[cfg(test)]`. Deps: super.

use super::{format_model_display, AttributionSource};

#[test]
fn model_display_preserves_attribution_and_substitution() {
    for (observed, requested, source, expected) in [
        (None, Some("gpt-5.6"), None, Some("gpt-5.6?")),
        (Some("gpt-5.6"), Some("gpt-5.6"), None, Some("gpt-5.6")),
        (Some("composer-2"), Some("auto"), None, Some("composer-2 (asked auto)")),
        (None, None, None, None),
        (
            Some("gpt-5.6"),
            Some("gpt-5.6"),
            Some(AttributionSource::ConfirmedBySuccess),
            Some("gpt-5.6 (inferred)"),
        ),
        (Some("gpt-5.6"), Some("gpt-5.6"), Some(AttributionSource::Echoed), Some("gpt-5.6")),
        (Some("composer-2"), Some("auto"), Some(AttributionSource::Echoed), Some("composer-2 (asked auto)")),
    ] {
        assert_eq!(format_model_display(observed, requested, source).as_deref(), expected, "case: {:?}",
            (observed, requested, source));
    }
}

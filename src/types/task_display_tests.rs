// Tests for model and route display helpers on Task.
// Loaded by task.rs under `#[cfg(test)]`. Deps: super.

use super::{format_model_display, AttributionSource};

#[test]
fn an_unconfirmed_request_is_marked_as_one() {
    assert_eq!(
        format_model_display(None, Some("gpt-5.6"), None),
        Some("gpt-5.6?".to_string())
    );
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

// Status guard helpers for Store mutation methods.
// Exports warn-only escape hatch predicate.
// Deps: std env.

pub(crate) fn status_guard_warn_only() -> bool {
    std::env::var("AID_STATUS_GUARD")
        .map(|value| value == "warn")
        .unwrap_or(false)
}

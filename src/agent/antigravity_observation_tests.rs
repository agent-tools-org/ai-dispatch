// Backend label attribution regressions for agy.
// Uses captured model-table columns and diagnostic text.
use super::*;
#[test]
fn selected_backend_label_maps_to_exact_served_id() {
    let log = "Propagating selected model override to backend: label=\"Gemini 4 Argon (Medium)\"";
    let label = selected_label(log).expect("label");
    assert_eq!(
        model_for_label("gemini-4-argon-medium\tGemini 4 Argon (Medium)", label),
        Some("gemini-4-argon-medium".into())
    );
    assert_eq!(
        model_for_label("gemini-4-argon-high\tGemini 4 Argon (High)", label),
        None
    );
}

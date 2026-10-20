// Tests for Grok model-list parsing.
// Keeps captured model output separate from completion parsing.

use super::parse_grok_models_output;

#[test]
fn parses_current_grok_models_output_and_ignores_footer_text() {
    let output = r#"You are logged in with grok.com.

Default model: grok-4.6

Available models:
  * grok-4.6 (default)
  - grok-4.5
"#;
    assert_eq!(parse_grok_models_output(output), ["grok-4.6", "grok-4.5"]);
    assert_eq!(
        parse_grok_models_output(&format!("{output}Footer text")),
        ["grok-4.6", "grok-4.5"]
    );
}

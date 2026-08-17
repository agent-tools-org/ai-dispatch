// Parser regression test using stdout captured from `agy models` on 2026-08-17.
// Exports: module-scoped test only.
// Deps: parse_agy_models_output and the captured text fixture.

#[test]
fn parses_captured_agy_models_output_exactly() {
    let models = super::parse_agy_models_output(include_str!("antigravity_models_fixture.txt"));
    let expected = [
        "gemini-3.7-flash-high",
        "gemini-3.7-flash-medium",
        "gemini-3.7-flash-low",
        "gemini-3.6-flash-high",
        "gemini-3.6-flash-medium",
        "gemini-3.6-flash-low",
        "gemini-3.5-flash-high",
        "gemini-3.5-flash-medium",
        "gemini-3.5-flash-low",
        "gemini-3.1-pro-high",
        "gemini-3.1-pro-low",
        "claude-sonnet-4-6",
        "claude-opus-4-6-thinking",
        "gpt-oss-120b-medium",
    ];
    assert_eq!(models, expected.map(str::to_string));
}

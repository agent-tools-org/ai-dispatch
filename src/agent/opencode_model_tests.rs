// Parser regression test using stdout captured from `opencode models` on 2026-08-18.
// Exports: module-scoped tests only.
// Deps: parse_opencode_models_output and the captured text fixture.

#[test]
fn parses_captured_opencode_models_output_exactly() {
    let fixture = include_str!("opencode_models_fixture.txt");
    let models = super::parse_opencode_models_output(fixture);
    let expected: Vec<String> = fixture
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(models, expected);
    assert_eq!(models.len(), 182);
    assert_eq!(models[0], "opencode/big-pickle");
    assert!(models.contains(&"opencode-go/glm-5.2".to_string()));
    assert_eq!(models.last().map(String::as_str), Some("ollama/qwen3:4b"));
}

#[test]
fn empty_or_error_output_yields_no_models() {
    assert!(super::parse_opencode_models_output("").is_empty());
    assert!(super::parse_opencode_models_output("ERROR: not logged in\n").is_empty());
    assert!(super::parse_opencode_models_output("Fetching available models...\n").is_empty());
}

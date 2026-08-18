// Parser regression test using stdout captured from `opencode models` on 2026-08-18.
// Exports: module-scoped tests only.
// Deps: parse_opencode_models_output, served_models_from_cli_output, captured text fixture.

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
fn captured_fixture_probe_fails_loudly_if_parser_returns_nothing() {
    let models = super::served_models_from_cli_output(include_str!("opencode_models_fixture.txt"))
        .expect("parser silently returned no models from captured opencode models output");
    assert!(
        models.len() > 5,
        "captured CLI output must grow the catalog past the five static opencode/* rows, got {}",
        models.len()
    );
    assert!(
        models.contains(&"opencode-go/glm-5.2".to_string()),
        "captured CLI output must include opencode-go/glm-5.2"
    );
}

#[test]
fn empty_or_error_output_is_a_failed_probe() {
    assert_eq!(super::served_models_from_cli_output(""), None);
    assert_eq!(
        super::served_models_from_cli_output("ERROR: not logged in\n"),
        None
    );
    assert_eq!(
        super::served_models_from_cli_output("Fetching available models...\n"),
        None
    );
}

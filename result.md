## Findings
No findings.

## Outcome
Implemented smart model routing for simple prompts in [/tmp/aid-wt-feat/hermes-inspired/h6-smart-routing/src/cmd/run_dispatch_resolve.rs] by applying cheap-model selection before budget-mode routing when no explicit `--model` is set and `selection.smart_routing` is enabled.

Added the `selection.smart_routing` config flag with a true default in [/tmp/aid-wt-feat/hermes-inspired/h6-smart-routing/src/config.rs], including coverage for both TOML parsing and `AidConfig::default()`.

Added the conservative `is_simple_for_routing()` heuristic and prompt-shape tests in [/tmp/aid-wt-feat/hermes-inspired/h6-smart-routing/src/agent/classifier.rs].

## Verification
- `cargo check -p ai-dispatch`
- `cargo test -p ai-dispatch --bin aid classifier::tests::`
- `cargo test -p ai-dispatch --bin aid selection_smart_routing_defaults_to_true`
- `bash /Users/mingsun/.aid/skills/implementer/scripts/check-file-size.sh src/agent/classifier.rs`

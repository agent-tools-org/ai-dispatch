## Findings
No findings.

## Result
Added Claude Code as a built-in `aid` agent with `AgentKind::Claude`, command construction for `claude -p --output-format stream-json --verbose --dangerously-skip-permissions`, real stream-json parsing, selection/scoring integration, fallback-chain support, config/profile/model entries, and runtime wiring.

Added targeted Claude adapter tests plus a selection test, and verified live execution with `aid run claude "say hello"` followed by `aid show` against an isolated writable `AID_HOME`.

## Verification
- `CARGO_TARGET_DIR=/tmp/cc-target-ai-dispatch-claude cargo check -p ai-dispatch`
- `CARGO_TARGET_DIR=/tmp/cc-target-ai-dispatch-claude cargo test -p ai-dispatch claude -- --nocapture`
- `AID_HOME=/tmp/aid-claude-check-2 CARGO_TARGET_DIR=/tmp/cc-target-ai-dispatch-claude cargo run -p ai-dispatch -- run claude "say hello"`
- `AID_HOME=/tmp/aid-claude-check-2 /tmp/cc-target-ai-dispatch-claude/debug/aid show t-de37`

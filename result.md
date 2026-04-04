## Findings
No findings.

## Verification
- `cargo check -p ai-dispatch`
- `cargo test -p ai-dispatch claude`
- `cargo test -p ai-dispatch write_code_is_complex_impl`
- `AID_HOME=$(mktemp -d) cargo run -q -p ai-dispatch -- run auto "write code" --dry-run`
  Evidence: the dry-run selected `claude`.
- The same dry-run against the existing local `AID_HOME` selected `codex` because stored success history and similar-task history still bias the selector toward Codex in this workspace; the isolated run confirms the new routing logic itself.

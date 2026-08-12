## Findings

- **High — In-agent recovery ran too late.** Cursor premium holds incorrectly cascaded to another agent. Recovery now runs before agent fallback, preserving Cursor and switching to `auto` (`src/cmd/run_dispatch_resolve.rs:234-269`).

- **Medium — Regression coverage was incomplete.** Added dispatch tests for Cursor premium recovery and OpenCode provider fallback (`src/cmd/run_dispatch_resolve_held_tests.rs`).

Markers still preserve full refusal messages and usable ISO reset timestamps. Unknown attribution remains agent-wide; Ollama remains provider-scoped.

## Verification

- Pre-fix: Cursor test selected `Codex` instead of `Cursor`.
- Full suite: 2,300 passed, 0 failed, 9 ignored.
- `aid build`: 0 errors.
- Commit: `71c04fc2`.
Implemented and committed as `5b434649`.

Changes:

- Bounded inbound-echo suppression to two matches within 5 seconds. The old single-use assertion was wrong because PTYs can emit both the terminal echo and the agent’s immediate repeat.
- Added regression coverage proving delayed identical text is real output.
- Required path evidence for OpenCode and Cursor file-write events.
- Added Grok/PTY timing coverage for 3-minute warning, 5-minute nudge, and 600-second hang detection.

Verification:

- Before: affected regression tests failed.
- After: `aid test --bin aid` — 2,297 passed, 0 failed.
- `aid build check -- --all-targets` — 0 errors.
Implemented and committed as `4b440f89`.

Changes:

- Echo suppression now allows two matches within 30 seconds, caps pending entries at 64, and treats later repeats as real output.
- Preserves delayed six-second PTY echoes while making hour-later repeats real.
- File-write detection now requires exactly one operand after `Writing`, `Creating`, or `wrote`, preserving `Makefile`, `Dockerfile`, and `CHANGELOG` while rejecting prose.
- Added tests for all reported false positives/negatives, third echoes, expiry, and memory bounds.

Before-fix failures included one-shot echo suppression and prose classified as `FileWrite`. After-fix:

- `aid build check -- --all-targets`: 0 errors
- `aid test --bin aid`: 2,300 passed, 0 failed, 9 ignored
- PTY tests: 51 passed
- Worktree clean.
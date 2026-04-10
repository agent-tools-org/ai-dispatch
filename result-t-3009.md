## Findings
No findings.

## Results
- Updated `auto_commit` to stage tracked changes with transient pathspec exclusions for `.aid-lock`, `result-*.md`, and `aid-batch-*.toml`.
- Added an empty-staging skip after tracked staging and untracked source staging.
- Factored untracked source staging so source-only tasks still commit before the empty-stage check while existing rescue behavior remains intact.
- Added regression tests for `.aid-lock`-only changes, real source changes, and tracked `result-*.md` exclusion.

## Verification
- `cargo check -p ai-dispatch` passed with existing warnings.
- `cargo test -p ai-dispatch --bin aid commit` passed.
- `src/commit.rs` is 260 lines and production code contains no `unwrap()` calls.

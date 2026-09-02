# aid Roadmap

This is the maintained execution order for the project. The authoritative item state lives in
`ai-board` (`ai-board item list --project ai-dispatch`); this document explains sequencing and
release boundaries.

## Current state

- Released on origin: **v10.37.0** (2026-08-19).
- Local `main` contains an unreleased 24-commit integration train for the authenticated fleet API
  and the macOS/iPadOS AID Command client, including live SSE data and latency acceptance probes.
- The current working tree completes `wi-4c47`: an agy process that reports a terminal executor
  error in its private diagnostic log fails even when the CLI exits 0 with partial stdout. Captured
  quota and network failures are the acceptance fixtures; a recovered tool error remains successful.
- The Rust web suite, 14-check live API probe, macOS build, iPadOS Simulator build, and macOS Swift
  test target pass locally. The probe lifecycle correction is tracked and closed as `wi-7b8e`.
- Cross-repository dispatch routing is completed as `wi-8ffc`: an explicit `--dir` now selects the
  target project's defaults, identity, agent Cargo cache, worktree seed, and verification cache.
- Older version labels such as the `v9.0 UX overhaul` epic are historical planning names, not future
  release numbers. They must be re-triaged before being scheduled.

## Next release gate

The independent review scope, evidence, and reproduction commands are frozen in
[the 2026-08-23 audit handoff](./audit-handoff-2026-08-23.md).

1. Review and commit the completed `wi-4c47`, `wi-7b8e`, and `wi-8ffc` working-tree changes.
2. Re-run the server gates on that exact release-candidate commit:
   `cargo test --features web --bin aid web::` and `scripts/probe-client-api.sh`.
3. Re-run both generated client schemes and the Swift test target on the same commit.
4. Cut the next release only after the Rust API and both client schemes pass from the same commit.

## Priority queue after the release

1. **Artifact custody without worktrees (`wi-dc6a`, high).** Rescue must never commit task result
   artifacts onto the principal's active integration branch.
2. **Project budget enforcement (`wi-29bd`, high).** Decide and document whether the configured cap
   refuses, warns, or downgrades a dispatch, then make the declared number enforce that contract.
3. **Merge conflict attribution (`wi-e1a0`, high).** Distinguish merge-level conflicts from stash
   restoration conflicts and report the recovery action for the actual failing layer.
4. **Truthful release dry-runs (`wi-5eef`, high).** `release.sh --dry-run` must fail on every
   inspectable condition that would fail the real release, including orphan hygiene.

## Longer-horizon programs

- Re-triage the remaining UX-debt epic `wi-5b7e` against current v10 behavior before implementing
  its old assumptions.
- Keep the batch lineage work (`wi-f4bf`), content-hash resume (`wi-1479`), and unified resource
  lifecycle (`wi-7804`) as separate slices with scenario-driven tests.
- Audit the five long-running H-series initiatives in `active` state. They have not been updated in
  more than four months and should not be treated as active delivery work until reconfirmed.

## Process

- Open every implementation task in `ai-board` and link its commits with the `wi-<id>` identifier.
- Keep one correctness claim per slice and capture the incident that motivated its regression test.
- Do not release a commit without creating and pushing its version tag immediately after the branch
  push.
- Generate release notes through `scripts/release.sh`; do not use this roadmap as a changelog.

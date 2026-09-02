# Audit Handoff — Current Roadmap Changes (2026-08-23)

This document freezes the local review context for an independent audit. It describes the
uncommitted roadmap work above local `main`, the committed release train below it, the evidence
already collected, and the remaining review decisions. It is not an audit approval.

## Review baseline

| Field | Value |
| --- | --- |
| Branch | `main` |
| Local HEAD | `06635b406ae4c35a9537c154a452b04fae9ae5c0` |
| `origin/main` | `f8999e447080003f84c49a900afedb5fd4c82488` |
| Committed divergence | Local HEAD is 24 commits ahead of `origin/main` |
| Primary audit diff | Working tree relative to local HEAD |
| Release state | No commit, push, or release tag has been created for the working-tree changes |
| Audit state | Independent audit is required before the changes are committed or released |

The release candidate has two review layers:

1. The committed client/API integration train from `origin/main` through local HEAD: 123 files,
   8,603 insertions, and 615 deletions. This is the base beneath the primary audit diff.
2. The uncommitted correctness work described below: 14 tracked files with 372 insertions and 301
   deletions, plus six new implementation/test fixture files. This handoff file is a documentation-
   only addition made after that count was captured.

## Uncommitted change slices

| Work item | Problem | Implemented contract | Acceptance evidence |
| --- | --- | --- | --- |
| `wi-4c47` | An agy child could exit 0 after reporting a terminal executor failure in its diagnostic log, causing a false-success task result. | The watcher treats the exact `agent executor error:` diagnostic marker as terminal failure even when stdout is partial and the process exits 0. Ordinary recovered tool errors remain successful. | Three real-child fixture cases passed; the 45-test watcher suite passed. |
| `wi-7b8e` | The live API probe launched the server through a subshell and cleaned up with broad process matching, so readiness and ownership were unreliable. | The probe owns one `SERVER_PID`, waits for authenticated readiness, and stops that exact process with `kill` followed by `wait`. | All 14 live API checks passed, including restart and latency checks. |
| `wi-8ffc` | Explicit cross-repository dispatch could inherit the caller repository's defaults and managed Cargo cache namespace. | Explicit `--dir` selects the target project configuration, identity, agent cache, worktree seed, and verification cache. No caller-config fallback is used when the explicit target has no config. | Two binary cross-project E2E cases, 18 dispatch preparation tests, and three Cargo layout tests passed. |

## File inventory

### Terminal diagnostic failure detection (`wi-4c47`)

- `src/agent/mod.rs` adds the agent-level terminal diagnostic contract.
- `src/agent/antigravity.rs` recognizes only the exact agy terminal executor marker.
- `src/watcher/buffered.rs` converts terminal diagnostics into a failed task event and applies quota
  handling only to a terminal marker.
- `src/watcher/buffered_completion_tests.rs` exercises real child-process completion paths.
- `tests/fixtures/agy-exit0-terminal-error.log` captures a quota-style terminal failure.
- `tests/fixtures/agy-exit0-terminal-network-error.log` captures a network terminal failure.
- `tests/fixtures/agy-recovered-tool-error.log` protects the successful recovery path.

### Live probe ownership (`wi-7b8e`)

- `scripts/probe-client-api.sh` owns, checks, and reaps the exact server process.

### Cross-project dispatch isolation (`wi-8ffc`)

- `src/agent/cargo_target_layout.rs` implements project-aware rewrites for recognized managed Cargo
  target layouts.
- `src/agent/env.rs` applies the target project namespace to agent caches and branch seeds.
- `src/agent/env_tests.rs` covers the project-aware environment contract.
- `src/cmd/run_dispatch_prepare.rs` selects target-project configuration and delegates worktree
  preparation.
- `src/cmd/run_dispatch_worktree.rs` contains the extracted worktree preparation flow.
- `src/cmd/run.rs` registers the extracted command module.
- `src/cmd/run_verify.rs` and `src/cmd/merge_verify.rs` select the target-project verification
  cache.
- `src/worktree_deps.rs` seeds dependencies from the target-project cache.
- `tests/project_isolation_e2e.rs` verifies the behavior through the compiled `aid` binary.
- `docs/shared-cargo-cache-measurements.md` records the managed cache contract and measurements.

### Release planning

- `docs/roadmap.md` replaces stale release assumptions with the current gate and follow-up order.
- `docs/audit-handoff-2026-08-23.md` is this audit handoff and contains no product behavior change.

## Behavioral boundaries to preserve

- Terminal failure detection is agent-specific and marker-specific. A generic tool error, a
  recovered error, or partial stdout alone must not turn a successful task into a failure.
- Diagnostic inspection must use the completed task's own log and must not consume stale output
  from another task.
- Probe cleanup must affect only the process started by the probe, including early-exit and failed-
  readiness paths.
- Explicit `--dir` is authoritative. If its target has no project configuration, caller defaults
  must not leak into it.
- Only recognized managed layouts such as `.cargo-target/<project>/...` and
  `cargo-target/<project>/...` are renamed. Arbitrary custom Cargo target roots remain unchanged.
- Rewriting replaces the caller project segment with the target project segment and removes a
  caller branch suffix. The target identifier is validated before it becomes a path segment.
- The dispatch preparation split is structural; dispatch behavior outside target-project selection
  must remain unchanged.
- No compatibility fallback or legacy path was added.

## Verification evidence already collected

| Gate | Result |
| --- | --- |
| Exact exit-0 diagnostic scenarios | 3 passed |
| Buffered watcher suite | 45 passed |
| Cross-project binary E2E | 2 passed |
| Dispatch preparation tests | 18 passed |
| Managed Cargo target layout tests | 3 passed |
| Final full Rust suite | 2,511 passed, 0 failed, 9 ignored (2,520 total) |
| Rust web suite | 32 passed |
| Live client API probe | 14 passed, 0 failed, 0 skipped |
| Latest probe latency | fleet 0.046381s; agents 0.024106s; tasks 0.029005s |
| macOS app scheme | Build passed |
| iPadOS Simulator app scheme | Build passed |
| macOS Swift test target | Passed |
| Patch hygiene | `git diff --check` passed |

The full Rust suite initially exposed a non-Rust verification-infrastructure regression: the
simulated verification environment lost its Cargo target evidence. The verification path was
changed to use the project-aware target unconditionally, the focused regression passed, and the
final full suite then passed.

No `cargo fmt` command was run, in accordance with repository policy.

## Suggested reproduction commands

Use a fresh temporary target directory where practical and record the exact commit under review.

```bash
cargo test --target-dir /tmp/ai-dispatch-audit-target --test project_isolation_e2e -- --nocapture
cargo test --target-dir /tmp/ai-dispatch-audit-target --bin aid
cargo test --target-dir /tmp/ai-dispatch-audit-target --features web --bin aid web::
cargo build --target-dir /tmp/ai-dispatch-audit-target --features web --bin aid
AID_BIN=/tmp/ai-dispatch-audit-target/debug/aid PROBE_PORT=18971 ./scripts/probe-client-api.sh
```

```bash
cd client && xcodegen generate && cd ..
xcodebuild -project client/AIDCommand.xcodeproj -scheme 'AIDCommand macOS' \
  -destination 'platform=macOS' -derivedDataPath /tmp/aid-client-audit build
xcodebuild -project client/AIDCommand.xcodeproj -scheme 'AIDCommand iPad' \
  -destination 'generic/platform=iOS Simulator' -derivedDataPath /tmp/aid-client-audit build
xcodebuild -project client/AIDCommand.xcodeproj -scheme 'AIDCommand macOS' \
  -destination 'platform=macOS' -derivedDataPath /tmp/aid-client-audit test
git diff --check
```

The live probe copies the configured store and requires its action preconditions. Any `SKIP` is a
failed acceptance run, not a pass.

## Auditor focus

1. Confirm terminal marker matching cannot create false positives or read stale diagnostics.
2. Confirm quota mark/clear behavior remains coherent for terminal and recovered errors.
3. Exercise probe cleanup after normal completion, readiness timeout, and an occupied port.
4. Review cache rewriting for nested paths, symlinks, invalid project identifiers, and custom roots.
5. Confirm the dispatch refactor did not alter worktree reuse, verification, or dependency seeding
   outside the explicit cross-project case.
6. Review the 24-commit committed integration train separately from the uncommitted three-slice
   diff; the test evidence covers their combined local state but does not replace code review.

## Known non-blocking observations

- The latest normal Rust build reported three pre-existing warnings; the test build reported eight.
- A clean client build previously reported two Swift warnings: an assigned value that is never read
  in `SVGPathParser.swift`, and a never-mutated local in `SettingsView.swift`.
- These warnings were not introduced or remediated by the three uncommitted slices and should not be
  silently treated as new audit findings.

## Audit decision record

The auditor should append or attach a result with the following fields:

- Decision: `PASS`, `PASS WITH FOLLOW-UPS`, or `FIX REQUIRED`
- Commit reviewed and whether uncommitted changes were included
- Commands rerun and their results
- Findings, with severity and affected work item
- Required fixes or accepted follow-ups
- Auditor and completion date

After approval, create coherent commits for `wi-4c47`, `wi-7b8e`, and `wi-8ffc`, rerun the release
gates on the exact candidate commit, and only then proceed with the release/tag workflow.

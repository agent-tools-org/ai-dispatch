# CLI constraint and execution audit

Date: 2026-09-05. Scope: all explicit clap `conflicts_with`, `requires`, and
argument-group declarations under `src/cli`, plus their run, watch, merge, and
message-input execution paths. Findings below distinguish repaired validation
defects from remaining behavior and design defects. This is a local source audit;
it does not claim exhaustive runtime coverage of every command or agent adapter.

## Changes completed in this patch

- Added task-independent CLI error history and `aid errors --limit N --json`.
  Parse failures are captured before database initialization. Known run-option
  rejections record every collected issue; other returned errors record a generic
  command-failure marker. Help/version remain successful, unlogged exits.
- Moved read-only/worktree rejection before task insertion. Its error now directs
  callers to `--read-only --dir <checkout-path>` for an existing checkout.
- Combined critical-proof, read-only/worktree, effective sandbox/container,
  audit/no-audit, and iteration checks after project defaults and before task
  insertion. Iteration configuration previously failed during postprocessing,
  after the agent could already have performed work.
- Closed the critical-proof presence-check loophole: empty, `none`, `false`, and
  `skip` verification values no longer count as enabled verification.
- Explained the distinction between a bug-audit task (`--kind debugging`),
  read-only access, and post-task cross-audit (`--audit`) in help and the guide.

## Remaining findings

### P1: Critical rigor is not enforced as an execution outcome

Trigger: request critical rigor with verification and audit flags, then run a
read-only task, or run where the `aic` binary is absent.

Evidence:

- [Run validation](../../src/command_diagnostics/run_options.rs) checks configured
  verification and audit, but does not establish that either check will execute.
- [Verification skip logic](../../src/cmd/run_verify_outcome.rs) explicitly skips
  read-only tasks. [Verification dispatch](../../src/cmd/run_verify.rs) also
  returns when no working directory is available; automatic verification can
  resolve to `skip` in [verify.rs](../../src/verify.rs).
- [Post-task audit](../../src/cmd/run_post.rs) records `skipped` when `aic` is
  missing, independently of declared rigor.
- [Task outcome](../../src/types/task.rs) does not consult `audit_verdict` or
  declared rigor. A missing or failed audit therefore does not itself make the
  derived task outcome unsuccessful. Verification has its own outcome handling;
  a skipped required check must not be assumed to have passed.

Impact: supplying flags can satisfy preflight without satisfying the promised
proof level. The original read-only audit also requires an audit of the audit,
which is a different operation from the requested review.

Recommended repair: define proof obligations for implementation and review
tasks separately; persist them and enforce them when deriving the outcome.
For an obligatory audit, establish availability before launching expensive work
and treat skipped/failed evidence as unfulfilled. Do not silently lower rigor.

### P1: Explicit merge selectors can silently change the selected target

Trigger: `aid merge <task-id> --group <group-id>`, optionally with `--lanes`.

[Merge arguments](../../src/cli/command_args_b.rs) accept both selectors.
[Merge dispatch](../../src/cmd/merge.rs) prefers the task in ordinary mode, but
the lanes branch selects the group first and ignores the task ID. Lanes mode
also drops the `approve` argument when calling `merge_group_lanes`.

Impact: changing the execution mode changes which explicit selector wins. This
is a target-selection ambiguity on a command that modifies repository state.

Recommended repair: reject simultaneous explicit task/group selectors, preserve
an explicit task's precedence over an inherited `AID_GROUP`, and make unsupported
lanes options explicit preflight errors. Resolve environment-derived group state
before checking group requirements: the current wrapper rejects `--lanes` before
calling `resolve_group`, so `AID_GROUP` cannot satisfy that requirement.

### P1: Best-of candidates inherit the same writable target

[Best-of dispatch](../../src/cmd/run_bestof.rs) clones the parent's arguments for
each candidate and enables background execution without assigning separate
worktree names or directories. [Run dispatch](../../src/cmd/run_dispatch.rs)
enters best-of before normal preparation.

With an explicit worktree, the first active candidate can hold the lease and
cause subsequent candidates to fail. Without a worktree, writable candidates
can share a directory. The concurrent-write risk is inferred from the dispatch
and target-selection paths; this audit did not launch competing write agents.
The parent also reaches candidate selection before normal combined validation,
so malformed requests can still generate repeated candidate failures.

Recommended repair: validate the parent once, allocate an independent checkout
per writable candidate, and bind evaluation to each candidate's checkout.
Read-only candidates may share an explicitly selected immutable snapshot.

### P2: Project policy depends on which validation phase reads it

[Profile validation](../../src/cmd_dispatch/run_profile.rs) calls
`detect_project()` using the caller's current directory. Later
[dispatch preparation](../../src/cmd/run_dispatch_prepare.rs) detects the project
from `--dir`, falling back to the current directory. It does not use `--repo` or
`--repo-root` for this step.

Impact: a request targeting another project can be rejected by the caller's
profile requirement, miss the target's requirement, or receive defaults from a
different repository. Separately, [repo-root resolution](../../src/repo_root.rs)
silently prefers `--repo-root` when both repo selectors are provided.

Recommended repair: resolve repository anchor, checkout, and effective working
directory once; detect the project once from that resolved context; apply its
defaults before validating all policy. Reject inconsistent explicit anchors.

### P2: Quiet watch normalization bypasses declared mode conflicts

[Parser declarations](../../src/cli/command_args_watch.rs) reject wait/stream
and wait/TUI combinations. [Main normalization](../../src/main.rs) sets
`wait = true` whenever the global quiet flag is set, after parsing. Therefore
`aid watch --stream -q` and `aid watch --tui -q` can produce combinations the
parser rejects when `--wait` is explicit.

[Watch dispatch](../../src/cmd_dispatch/display.rs) selects TUI, then stream,
then wait, masking the contradiction by precedence. A test named
`watch_stream_conflicts_with_quiet` actually tests `--stream --wait`, leaving the
quiet-normalization path uncovered.

Recommended repair: make quiet affect output only, or resolve any wait shorthand
before validating modes. Test the exact shorthand combinations.

### P2: Message input sources silently override one another

`respond <task> <input> --file <file>` and
`reply <task> <message> --file <file>` parse successfully.
[Respond](../../src/cmd/respond.rs) and [reply](../../src/cmd/reply.rs) select
file contents first and discard the positional message. By comparison,
`retry --feedback --feedback-file` is explicitly rejected by clap.

Recommended repair: use the same exactly-one-source contract for message
commands. Any intended stdin behavior should be explicit in that contract.

### P2: Verification values have inconsistent meanings across layers

[Outcome classification](../../src/types/outcome.rs) treats empty, `none`,
`false`, and `skip` as disabled verification. [Command construction](../../src/verify.rs)
only special-cases `skip`; other strings become commands (or fail parsing).
For example, `false` is a real failing command, despite being classified as
verification-disabled elsewhere. The patch consistently uses the existing
outcome contract for critical preflight but does not redesign these values.

Recommended repair: represent verification mode separately from the command
string. A command named `false` must retain its normal command semantics.

### P2: Output modifiers use inconsistent conflict rules

`show --full --brief` is rejected, but `output --full --brief` is accepted by
[output arguments](../../src/cli/command_args_c.rs); the
[dispatch match](../../src/cmd_dispatch/dispatch_match.rs) discards `full` and
forwards only `brief`. Likewise, `board --stream --json` accepts both output
modes but [board dispatch](../../src/cmd_dispatch/display.rs) enters streaming
without forwarding `json`. Export's `--sharegpt` also selects a different output
path while the independent `--format` value is accepted.

Recommended repair: use a single explicit format/mode selector per command and
consistent full/brief modifiers. When combinations are intended, document their
meaning and test the resulting output shape rather than relying on precedence.

## Declared constraint inventory

| Surface | Declared relation | Assessment |
|---|---|---|
| run | eval and eval-feedback-template require iterate | Correct dependency; reverse requirement, positive count, and non-empty eval are now checked before task creation. |
| run | metric requires best-of | Parser dependency is valid; candidate target isolation and parent validation remain open. |
| run | no-skill conflicts with skill | Clear explicit override contract; retained. |
| run | container conflicts with sandbox | Retained; effective values are now checked after project defaults too. |
| run | no-audit conflicts with audit | Clear explicit override contract; critical's execution guarantee remains open. |
| run | prompt and prompt-file | Runtime-only exclusivity, with an error naming a nonexistent `--prompt` flag. Move input-source validation ahead of routing and name the positional prompt accurately. |
| watch | TUI vs wait, stream, exit-on-await, timeout; stream vs wait and exit-on-await | Explicit relations are coherent; quiet normalization can bypass them. |
| show | one display mode among events/result/json/context/explain/summary/diff/output/transcript/log | Coherent output-mode group; retained. |
| show | file and branch require diff; brief conflicts with full | Coherent modifiers; retained. |
| cost | group, summary, agent mutually exclusive | Coherent report-mode selectors; retained. |
| changelog | version conflicts with all/count; all conflicts with version | Explicit count cannot combine with version; the default count does not constitute an explicit conflict. |
| retry | feedback conflicts with feedback-file | Clear input-source contract; should be reused by reply/respond. |
| agent config | enable conflicts with disable | Coherent boolean override contract; retained. |
| merge | lanes requires group; lanes rejects check/target in handlers | Late validation plus selector precedence remains open; explicit selector rules belong in preflight. |

## Validation evidence

The original five new CLI tests failed against the baseline, reproducing missing
history, missing audit-kind guidance, and one-error-at-a-time rejection.
The implemented patch passes eight rejection-history E2E tests, three target
default/checkout E2E tests, four existing advice E2E tests, and twelve official
guide E2E tests, and three guide-installation E2E tests. The existing CLI-filter
regression suite also passes 61 tests. Coverage includes 12 concurrent parser failures, argument
redaction, preserving parser exit code 2 when logging fails, help/version,
zero task rows for combined validation failure, inherited container conflicts,
valid critical defaults, and preservation of an explicit read-only checkout.

Remaining findings are source-backed review results, not claims that they have
been repaired. Recommended repair order: proof outcomes and merge selection,
candidate isolation, unified target/project context, then consistent mode and
input-source validation.

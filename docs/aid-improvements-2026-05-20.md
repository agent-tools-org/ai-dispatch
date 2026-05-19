# aid Improvement Research — 2026-05-20

Current release: **v8.99.10** (2026-05-13). Prior planning doc (`docs/ux-debt.md` / `docs/roadmap.md`) was last updated at v8.94.0 and is 6 releases stale. This research closes that gap, surfaces five new code-level findings from a deep codex-driven survey, and proposes a prioritized v9.0 plan.

Companion artifact: [`docs/research/2026-05-20-code-survey-codex.md`](research/2026-05-20-code-survey-codex.md) — verbatim 5-finding survey with full file:line citations.

---

## Executive Summary

1. **Codex's own sandbox flag is never set by aid.** `sandbox=false` in a spec only disables aid's Apple-Container wrapper. The codex CLI is launched with `--full-auto` and *no* `-s` flag, so write permission falls back to whatever codex's profile/default decides. Real-world impact: 2026-05-20 morpho dispatch (`refactor/bid-model-v2`) produced zero `src/` changes despite `sandbox=false` and `read_only=false`.
2. **Audit-report classifier is silently overbroad.** `report_mode::is_audit_report_task` lowercases the prompt and does substring matching on `audit`, `review`, `cross-audit`, `code review`, `peer review`. "Add an audit log feature", "Review and implement the fix", "Redesign the audit subsystem" all auto-mutate the prompt to demand a Markdown report. No surface in `aid show` tells the user this happened.
3. **Cross-agent permission semantics are inconsistent.** The same `read_only` flag changes Cursor mode, Gemini approval, OpenCode prompt text, and means nothing to Codex's CLI sandbox. Specs are impossible to reason about across agents.
4. **Delivery assessment exists but the default text board hides it.** `delivery_assessment={empty_diff,hollow_output}` is computed and stored, surfaced in `aid tree` and `aid show --json`, but `aid board` (the primary monitoring surface) renders only the bare `DONE` status. A 0-diff task with $18 in tokens looks identical to a finished implementation.
5. **aid observes artifacts, not intent.** There is no `expected_deliverable` field. A code-implementation prompt that finishes with only `result.md` and zero diff cannot be distinguished from a research task that legitimately produced a report.

These five form the **v9.0 must-fix core**. The remaining 14 items from `docs/ux-debt.md` and 6 items still tracked in memory are layered onto the v9.x roadmap below.

---

## Method

- **Self (Claude):** read 8 aid-relevant memory files, `docs/ux-debt.md`, `docs/roadmap.md`, `DESIGN.md`, CHANGELOG entries for v8.95.0–v8.99.10.
- **codex (`aid run` t-c401, 6m 01s, $18.88, 5M tokens):** deep code survey of sandbox flag plumbing, audit-report detection, and result-vs-diff observability. Produced 5 findings with file:line citations — preserved verbatim in companion artifact.
- **gemini (`aid run` t-3462, 1m 25s, $0.31):** intended pain-archaeology over memory + CHANGELOG. Blocked by gemini's workspace-path sandbox before it could write `/tmp/aid-research-pain.md` — itself a data point for finding #3 below.
- **opencode (`aid run` t-3a4a, 1s):** intended ecosystem-capability comparison. Failed in 1 s with `ProviderModelNotFoundError: opencode/mimo-v2-flash-free` yet recorded as `DONE` — itself a data point for finding #4 ("the board does not lie" violation, already in the v9.0 principles).

The codex run is the substantive backbone. The gemini and opencode failures are themselves evidence and are folded into the findings.

---

## Part A — New code-level findings (from codex survey)

Severity, current behavior, gap, and recommendations excerpted; full file:line citations in the companion artifact.

### A1. Codex `--sandbox` not threaded through (HIGH)

| Aspect | Detail |
|---|---|
| Today | `sandbox=false` only skips `wrap_command` in `src/sandbox.rs:46` (aid's Apple Container). Codex command is `codex exec --json --skip-git-repo-check --full-auto <prompt>` at `src/agent/codex.rs:83-84`. No `-s`, no `--add-dir`, no `--approval-mode` is ever sent. |
| Gap | aid conflates two different concepts under `sandbox`: aid-managed container vs. agent-side CLI sandbox. `read_only=false` is not translated into a positive codex write policy. |
| Fix (P0) | In `src/agent/codex.rs`, map `opts.read_only == true → -s read-only`, `opts.read_only == false → -s workspace-write`. Add adapter tests asserting both flags. |
| Fix (P0) | Split `RunArgs::sandbox` into `container_sandbox: bool` + `agent_sandbox: Option<AgentSandboxMode>`. Stop overloading `sandbox=false` as an implicit write-permission signal. |
| Fix (P1) | `src/cmd/run_validate.rs` warns when `agent=codex && read_only=false && no explicit codex sandbox mode set`. |

### A2. Cross-agent permission inconsistency (MED)

| Agent | What `read_only=true` actually does | Container sandbox supported? |
|---|---|---|
| Cursor | `--mode plan` (CLI-enforced) | yes |
| Gemini | `--approval-mode plan` (CLI-enforced) | yes |
| Codex | prompt text only; CLI defaults unchanged | yes |
| OpenCode | prompt text only; warning emitted that it is **not** enforced (`src/agent/opencode.rs:25-40`) | **no** (`src/sandbox.rs:11-21` excludes it) |

**Fix (P1):** Introduce `AgentPermissionSupport { enforced_read_only, write_mode_flag, container_supported, trust_flag }` per adapter. Surface the effective plan in dry-run and as a task event: `Permissions: aid_container=false, agent_sandbox=workspace-write, trust=true`.

**Fix (P2):** For OpenCode, either wire OpenCode's permission flag (if it has one) or make `read_only=true` reject unless a worktree is supplied — the code already knows it isn't enforceable.

### A3. Audit-report classifier overbroad (MED → HIGH given user-visible impact)

Defined in `src/cmd/report_mode.rs`:

- Explicit terms (trigger regardless of `read_only`): `audit`, `cross-audit`, `cross audit`, `adversarial audit`, `review`, `code review`, `peer review`.
- Structured-finding terms (require `read_only=true` + category in `Research|Documentation|Debugging`): `findings`, `pass/fail`, `severity`, `evidence`, `open questions`.
- Detection is substring on lowercased prompt. No word-boundary check.

**Confirmed false-positive triggers** (codex enumerated):

- "Add an audit log feature" → triggers via `audit`. (`src/batch/warnings.rs:84-86` already excludes `audit log` / `audit trail` for batch warnings but `report_mode` does not.)
- "Review and implement the requested fix" → triggers via `review`.
- "Redesign the audit subsystem" → triggers via `audit`.
- "Investigate and fix the crash" — `investigate` alone doesn't trigger, but combined with `read_only=true` + any structured term it does.

**Effect when triggered:** aid auto-sets `args.result_file = Some("result.md")` (`src/cmd/report_mode.rs:43-50`), injects `<aid-result-file>` instruction, appends "produce a Markdown audit report starting with `## Findings`". The agent obediently writes a report and skips code changes — even when the prompt asked for code.

**Fix (P0):** Require `read_only=true` OR explicit `result_file` OR a new explicit `audit_report=true` field before auto result-file mode kicks in. Add exclusions for "audit log/trail" + implementation verbs near `audit`/`review`.

**Fix (P1):** Replace substring matching with the word-boundary helper at `src/agent/classifier.rs:212-223`. Add false-positive regression tests for the prompts above.

**Fix (P1):** Persist `task.delivery_mode = code | report | audit_report` so `aid show` and `aid board` can render *why* a task ended up in report mode.

### A4. Delivery assessment hidden from text `aid board` (MED)

`delivery_assessment` (`src/types/delivery.rs:7-31`) already encodes `empty_diff` and `hollow_output`. It's persisted, exposed in `aid show --json`, `aid board --json`, and `aid tree` (`src/cmd/tree.rs:80-87`). But the **default text** `aid board` (`src/board.rs:355-369`) renders only the bare status.

A second observability hole: foreground streaming success exit codes are written to the completion event text but **not** stored in `tasks.exit_code` (`src/watcher.rs:151-180`). Background PTY does store it (`src/pty_watch.rs:447-454`).

**Fix (P0):** Append `[delivery:empty_diff]` / `[delivery:hollow_output]` to terminal-task lines in `src/board.rs:355`.

**Fix (P0):** After `child.wait().await?` in `src/watcher.rs`, set `info.exit_code = exit_status.code()` before returning.

**Fix (P1):** Extend `empty_diff` detection to in-place (non-worktree) tasks via `start_sha..HEAD` diff. Don't skip empty-diff assessment just because verification ran — a green test suite + zero diff is still suspicious.

### A5. Intent-vs-artifact mismatch unobserved (HIGH)

aid currently observes artifacts: there is a `result.md`, there is or isn't a diff. It cannot answer "*was this task supposed to produce code?*"

**Fix (P0):** At dispatch time, derive an `expected_deliverable` from explicit flags + classifier:
- `result_file` or report-mode set → `report_expected`
- categories `Implementation|Refactor|Testing|SimpleEdit` && `!read_only` → `code_change_expected`
- `read_only=true` → `no_code_change_expected`

**Fix (P1):** On completion, cross-check expected vs observed:
- `expected=code_change && diff_empty && result_file_present` → new `delivery_assessment=report_only_for_code_task` warning.

**Fix (P1):** Store report-mode activation **separately** from `result_file`, so user-supplied `result_file` is not conflated with auto-injected audit reports.

---

## Part B — `docs/ux-debt.md` open items, re-triaged

`docs/ux-debt.md` listed 14 open items under "v9.0 UX overhaul". Status after reading v8.95–v8.99 CHANGELOG:

| # | Item | Sev | Still open? | Notes |
|---|---|---|---|---|
| B1 | `depends_on` doesn't rebase child branches onto parent's output | HIGH | OPEN | `roadmap.md` already accepts this as a v9.0 breaking change |
| B2 | Cross-branch semantic coupling not caught (`--analyze` is lexical) | HIGH | OPEN | Hard; needs symbol-level overlap |
| B3 | `aid merge` auto-stash traps conflicts in stash | HIGH | OPEN | — |
| B4 | GitButler hook vs `aid merge` asymmetry | HIGH | PARTIAL | v8.94.0 lane-merge path helps; manual conflict resolution still hits pre-commit |
| B5 | Batch failures don't deduplicate on retry | HIGH | OPEN | "Resume by content hash" already in roadmap.md as a v9.x candidate |
| B6 | No shared resource lifecycle (Resource trait) | HIGH | OPEN | Foundational; v8.98/v8.99 worktree relocation + atomic lock are partial down-payments |
| B7 | Errors surface at OS layer, not config layer | MED | PARTIAL | `dir = "."` resolution fixed v8.94.0; principle not generalized |
| B8 | `--analyze` warns but doesn't enforce | MED | OPEN | Needs `--strict-analyze` |
| B9 | `aid group delete` prompt language | MED | LIKELY FIXED | `--cascade` shipped v8.85; verify message in smoke test |
| B10 | Update-check banner overrides JSON output | MED | NEEDS VERIFY | Check `aid board --json` output is clean |
| B11 | Auto-injected `implementer` skill on research tasks | MED | OPEN | Tied to A3 (classifier-before-skill-injection) |
| B12 | `aid board --limit` hint | LOW | LIKELY FIXED | v8.46.0 added hint; verify |
| B13 | `aid show --diff` against merged task shows huge diffs | LOW | OPEN | Per-task `start_sha` exists (v8.67.0); diff base needs updating |
| B14 | TaskStatus variants inconsistent across display code | LOW | OPEN | `_ =>` fallthroughs are the symptom; needs `#[deny(non_exhaustive_omitted_patterns)]` or equivalent discipline |

Three items (B9, B10, B12) should be verified-and-closed in this cycle before v9.0 planning treats them as still-pending.

---

## Part C — Recurring themes in v8.95–v8.99 (the 6 versions since ux-debt.md was written)

The CHANGELOG between v8.95.0 and v8.99.10 reveals four recurring themes worth surfacing as systemic risk areas:

### C1. Worktree primitives are fragile under concurrency (HIGH)

Four releases in ~3 weeks touched worktree state:

- v8.95.0 `fix(worktree)`: re-anchor reused worktrees when an agent ran `git checkout`
- v8.98.0 `feat(worktree)`: relocate from `/tmp/aid-wt-*` to `~/.aid/worktrees/`
- v8.99.0 `fix(worktree)`: prune skips active worktrees; expose `--json`/`--active`
- v8.99.10 `fix(worktree)`: atomic lock acquisition (3 rounds of cross-audit closed TOCTOU + partial-write + cleanup race)

This is healthy *responsiveness*, but it's also evidence that the worktree state machine doesn't have property-based concurrency tests. **Recommendation:** invest in a multi-process race-test harness (`src/worktree/lock_tests.rs` already runs threaded; extend to forked processes simulating real parallel `aid run` invocations).

### C2. Watcher loop-detector keys (MED)

- v8.99.3: codex bursty file-writes 80-char-truncated to same prefix → false-positive loop kill (#125)
- v8.99.4: codex command included in loop-detector key for ToolCall events
- v8.99.8: droid tool args + metadata.command included
- v8.96.0: droid duplicate ToolCall events (tool_result/tool_use already paired)

Pattern: per-agent `raw_event_key` coverage was patchwork. MEMORY `project_aid_bugs.md` already notes claude/gemini/cursor/opencode/oz adapters still rely on truncated `detail` for non-FileWrite events. **Recommendation:** unify into a single trait method `Adapter::loop_detector_key(event) -> String` with per-agent tests asserting uniqueness across path/command/pattern dimensions.

### C3. BYOK / custom-agent permission plumbing (MED)

v8.99.2 (`aid byok` subcommand), v8.99.5 (gemini latest-model auto-detect), v8.99.6 (custom-agent `kind()` fix + opencode overlay + audit-result missing banner), v8.99.7 (droid `--skip-permissions-unsafe` default), v8.99.9 (MiMo manifest output cap fix).

Pattern: each provider integration discovers a different permission/output-cap edge. **Recommendation:** define a `BYOKManifest` contract that includes explicit `permission_mode_flag` and `output_token_max` fields, validated at `aid byok apply` time against a known schema.

### C4. Audit-report scaffolding partially built but classifier still naive

v8.99.6 added `report_mode::prompt_is_audit_report()` centralization and the "Structured audit result missing" retry banner. That's exactly the scaffolding finding A3 wants to *strengthen* — the substring matcher is the unfinished half.

---

## Part D — Items already in memory but not in `docs/ux-debt.md`

Promoting these from memory to the public backlog (also creating `ai-board` items in a follow-up step):

| ID | Item | Sev | Source |
|---|---|---|---|
| D1 | `aid batch retry <wg>` loses `depends_on` ordering — re-dispatched tasks fire concurrently | HIGH | `project_aid_bugs.md`; 2026-04-10 incident |
| D2 | aid post-run auto-commit scoops `.aid-lock` + `result-<task-id>.md` into stray commits | MED | `project_aid_bugs.md`; observed 4× in gitbutler rollout |
| D3 | Template leak in agent commit messages (markdown bullets from implementer/audit skill) | LOW | `project_template_leak_commit_msg.md`; partial fix v8.99.1 still incomplete for skill-injected prefixes |
| D4 | LoopDetector `raw_key` only proper for codex + FileWrite/ToolCall (other agents fall back to truncated detail) | LOW | `project_aid_bugs.md` |
| D5 | Batch worktree sharing for `depends_on` linear chains rejected by `validate_no_file_overlap()` | LOW | `project_batch_worktree_sharing.md`; workaround = `worktree_prefix` |
| D6 | Read-only tasks can't write their own `result_file` | MED | `feedback_readonly_result_file.md` |

D6 is particularly relevant: it interacts with A3 (audit-report mode auto-sets result_file). If audit-report mode also sets `read_only=true` in some future iteration, this becomes blocking.

---

## Proposed v9.0 prioritization

### v9.0.0 — UX overhaul (breaking)

The five v9.0 principles in `roadmap.md` remain authoritative. The new code-level findings layer on cleanly:

**Must-have (P0):**

1. **A1** — Codex `-s workspace-write` / `-s read-only` mapping; split `RunArgs::sandbox` into `container_sandbox` + `agent_sandbox`.
2. **A3** — Tighten audit-report classifier: require `read_only=true` OR explicit `result_file` OR new `audit_report=true` flag; word-boundary matching; false-positive regression tests.
3. **A4** — Show `delivery_assessment` in text `aid board`; store foreground streaming `exit_code` in tasks table.
4. **A5** — Persist `expected_deliverable` at dispatch; cross-check at completion; emit `report_only_for_code_task` warning.
5. **B1** — `depends_on` child branches start from parent's output (already announced breaking change).

**High-priority (P1):**

6. **A2** — `AgentPermissionSupport` per-adapter table; effective-plan event before launch.
7. **D1** — Batch retry preserves `depends_on` ordering.
8. **D2** — Auto-commit excludes `.aid-lock` / `result-*.md`.
9. **B7** — Generalize "errors translate to config layer" principle beyond `dir = "."`.
10. **B11** — Skill injection runs *after* task classification, so research tasks don't get `implementer`.

**Verify-and-close before v9.0 (P2):**

11. **B9, B10, B12** — confirm fixed; if so, remove from open list and update `docs/ux-debt.md`.

### v9.x — backlog

- **B2** — semantic overlap analysis in `--analyze` (large, parser-driven).
- **B3, B4** — `aid merge` conflict resolution path through non-workspace branch.
- **B5** — Batch resume by content hash.
- **B6** — `Resource` trait for lock/worktree/group/task-row lifecycle.
- **B8** — `--strict-analyze`.
- **B13** — Diff base from `start_sha` for merged tasks.
- **B14** — TaskStatus exhaustiveness enforcement.
- **C1, C2, C3** — concurrency test harness, unified loop-detector key trait, BYOK schema validation.
- **D3, D4, D5, D6** — per-item triage.

### v9.x — Hermes-inspired research (already in `project_v12_improvements.md`)

H1–H7 (context compression, credential pool, prompt injection scanning, training trajectory export, skills marketplace, smart routing, insights dashboard) remain forward-looking and orthogonal to the UX overhaul.

---

## Action items

1. **Confirm prioritization with boss** — this report's "Must-have" list is the proposed v9.0.0 scope; needs sign-off before any code work.
2. **Create `ai-board` items** for A1–A5 and D1–D6 (B-items already implicit in v9.0 epic `wi-5b7e`).
3. **Verify-and-close B9/B10/B12** — quick smoke tests; trim `docs/ux-debt.md` accordingly.
4. **Update `docs/ux-debt.md` and `docs/roadmap.md`** to absorb findings A1–A5 (currently only post-v8.94.0 fixes are reflected).
5. **Write false-positive regression tests for `is_audit_report_task`** as the lowest-cost down-payment on A3 — these tests will fail today and lock the desired behavior for the eventual fix.
6. **Tooling debt surfaced by this very research session:**
   - opencode dispatch with non-existent BYOK model exits 0 + status DONE (1 s, no work done). Filing as a new item under C3.
   - gemini dispatch can't write `/tmp/` paths; should fail loudly or auto-redirect to the task artifact directory. Filing under A2.

## Unreleased
- Transient cooldown no longer applies scoring −10: `is_rate_limited` is now the same Held check as `aid run`, so a bare 429 is not treated as a hold.
- Grok's `usage balance exhausted` 402 and Cursor's premium `you're out of usage` are Windowed holds: a newer dated aidbar snapshot with headroom can release them. A percentage without a date cannot. OpenCode prepaid, Gemini `IneligibleTier`, and Copilot's undated monthly/premium needles stay person-only. Cursor premium matches the Plan window only — On-demand is ignored for that group.


## v10.31.0 (2026-08-14)
- Model validation now actually fires for slow CLIs. The served-model probe capped at 2 seconds while `agy models` takes 3.9s and `grok models` 2.3-2.6s, so the probe always timed out, returned nothing, and aid accepted any model name at all — a nonexistent model dispatched without a word. The probe now allows 10 seconds and caches its result on disk for 24 hours, so the cost is paid once rather than on every dispatch.
- A model absent from the cached list triggers one refresh before aid rejects it, so a model the CLI gained since the last probe is accepted instead of being wrongly refused for a day.
- Probe output no longer mixes stderr into the model list. Agy's rejection message used to offer `ERROR` and `error` as served models.
- The served-model disk cache is written atomically, so two concurrent aid processes cannot leave a torn file behind.
- Fallback cargo target directories are reclaimed when their task is accepted and garbage-collected, and by `aid clean --worktrees`. A directory is removed only when the working directory it was keyed from no longer exists, so a build running in the main repository is never touched. When nothing is reclaimed, aid now says how many targets it held back and why.


## v10.30.0 (2026-08-13)
- One provider running out of credit no longer takes its whole CLI offline: a refusal is attributed to the route aid actually dispatched, so a sibling provider that is still serving stays available.
- A model aid picked for you is no longer mistaken for one you named. Stale catalog entries degrade to the agent's own default with a warning, while a model you asked for by name still fails loudly if the CLI does not serve it — on first dispatch and on retry alike.
- Retrying a task now replays the directory the original run actually used, instead of whichever directory you happened to be standing in. Agents that key their saved sessions by working directory can resume again.
- A fast failure whose only output went to stderr no longer leaves an empty log; the reason is preserved, bounded, and only for agents that exited on their own.
- New: docs/multimodal-capability-matrix.md records, per agent, which modalities are measured, which are unsupported, and which are simply unknown.


## v10.29.0 (2026-08-13)
- A sandbox that refuses writes to the cargo target directory no longer reads as a broken delivery: Rust verification now runs through the same cargo runner as `aid build` and `aid test`, so it retries against a writable temporary target and reports what the tests actually did.
- When a build environment genuinely cannot run, the task is recorded as unverified infrastructure rather than failed, so the board stops showing an agent's passing work as broken. A test that really fails is still a failure.
- Known follow-up: every rescued verification writes into the shared fallback target directory, which has no garbage collection and grows by roughly two gigabytes per branch.


## v10.28.0 (2026-08-13)
- Dispatching to grok works again: aid resolved grok-4.5 from a catalog that had gone stale against the CLI's own model list, and its served-model probe dropped every non-default row, so aid refused the very model it had just picked and no `aid run grok` could start at any budget level.
- aid now knows grok-4.6, and both grok rows stay on the unpriced `unknown` tier, so asking for the cheapest option can no longer strand budget selection on a model aid never chose.
- A model aid picked for you no longer turns a stale catalog into a dead dispatch: it is dropped with a warning and the agent's own default runs instead. A model you asked for by name still fails loudly if the CLI does not serve it.
- Recorded why an `aid retry` of a qwen task dies in two seconds, and why the outer sandbox turns a passing verification into a reported code failure.


## v10.27.0 (2026-08-13)
- Two ways aid could show you the wrong thing about your own work are closed: a report belonging to another task, and a merge restoring another task's tree
- `aid show --output` only renders a file it can prove belongs to the task you asked about. A relative `-o` path used to be resolved against the process working directory, so running the command from a directory that happened to hold a `report.md` displayed that file as the task's report — real work from a different task, with nothing marking it as foreign. Relative paths now resolve only under the task's own recorded directory, worktree, or task directory, and are required to stay inside it. When no owned file exists, that is stated and the task's own log is shown instead. `aid judge`, `--context-from` and summary conclusions share the same rule, so a caller who believes they passed a previous task's report into a prompt no longer receives something else in silence.
- Tasks dispatched before this release recorded no directory, so the migration backfills it from each task's stored dispatch arguments rather than leaving historical reports unreachable. One unreadable row can no longer stop the migration or the rest of the backfill, and it can no longer prevent aid from opening its database at all.
- `aid merge` no longer restores a stash that is not its own. It used to `git stash pop`, which takes the top of the stack — so a stash pushed in between, including one from another task's rescue, was restored onto the branch you were integrating on. The stash is now identified by a token aid generates and applied by commit id, never by stack position, and entries are left in your normal `git stash list` rather than dropped. A failed `git status` is treated as an error rather than as a clean tree, a conflicted merge keeps its own result and leaves your untracked files in place while you resolve it, and every failure after the point where your work leaves the worktree names the handle you can recover it with.


## v10.26.0 (2026-08-13)
- One provider running out of credit no longer writes off an entire CLI, and the test suite stopped flaking the release gate
- An opencode refusal is now attributed to the provider it came from. The provider is derived from the model string's prefix, so every provider the CLI serves is covered rather than a list someone wrote down — `opencode models` currently serves five, and the largest by model count was missing from an earlier hardcoded attempt. A refusal that cannot be attributed still holds the whole agent, and a refusal naming `unknown` counts as unattributed rather than as a provider called unknown. Holds are applied after in-agent recovery, so an agent that meters tiers separately (cursor) still switches premium to auto and keeps the agent instead of cascading away.
- Rate-limit markers keep the provider's full refusal message instead of truncating it to 200 characters, and an ISO reset timestamp in the refusal now populates `recovery_at` so the hold can expire on its own. A refusal that carries no usable reset information leaves `recovery_at` unknown rather than guessing one.
- Four tests that failed under concurrent load and passed in isolation are fixed at cause, not by widening their timeouts. `aid reply` injects its poll sleep so the test no longer races a 25ms wall-clock budget; the streaming watcher flushes its log before returning so a reader sees the final line; the verify test polls until the grandchild process is gone instead of sleeping and hoping; the PTY idle test emits above the monitor's poll interval so the idle check it is named after actually runs, and `idle_hang_elapsed` takes an injectable clock so its 600-second boundary is pinned rather than raced. Each is proven by mutation: break the behaviour, the test fails.


## v10.25.0 (2026-08-12)
- Cancelling a task no longer counts against the agent, and aid's own idle nudge no longer masks a hang
- Operator cancellation is recorded as `Stopped`, not `Failed`. Ctrl-C, SIGTERM and `aid stop` all take this path, and every consumer that derives a success rate, a failure count or a webhook status was updated to match, so `aid stats` no longer charges your interruptions to the agent. Genuine failures — crashes, non-zero exits, timeouts, reaper kills, verify failures — still count as failures. Webhooks now receive a distinct `stopped` status instead of silently receiving nothing.
- aid's own idle nudge can no longer be mistaken for agent progress. Inbound echo suppression is bounded by a 30-second window, two matches and a 64-entry cap instead of being consumed by the first match, so the PTY's echo and the agent's immediate repeat are both absorbed while later identical output still counts as real progress. Previously the second echo reset the activity clock, which could keep `hung_detected` from ever firing on a stalled task.
- `aid retry` gained `--model`, `--idle-timeout` and `--feedback-file`/`-F`. Each inherits the original task's value when unspecified. `--feedback` and `--feedback-file` are mutually exclusive, and passing both is an error rather than a silent precedence rule.


## v10.24.0 (2026-08-12)
- Agent reports now read as what the agent actually wrote, and the TUI keeps your place when new tasks arrive
- `aid show --output` no longer corrupts non-ASCII text: ANSI stripping walked the line byte by byte and destroyed every multi-byte UTF-8 sequence, turning an apostrophe into mojibake on 141 of the last 351 tasks. It now reuses the escape stripper that already handled this correctly
- Agent response envelopes are unwrapped for persisted reports: a grok report reached the operator as a single-line JSON blob with escaped newlines on 69 of about 80 recent tasks, because only the gemini extractor was ever tried. Extraction is now agent-aware and shared, and `aid show` also unwraps artifacts written by older versions
- The TUI keeps the task you are looking at: pane focus, per-pane scroll offsets and tree selection are keyed by task identity instead of list position, so a new task arriving or a task starting to run no longer moves the view out from under you. The list stays free to reorder
- The bottom status bar now carries running and total counts, failed count, aggregate agent CPU and memory, and the active filter scope, from state the refresh cycle already holds and without a database query per frame. Three duplicated footer implementations were replaced by one shared builder
- New task statistics view with a time-range selector, activity heatmap with streaks, token trend with its peak, and ranked per-project rollups. The previous cost, success-rate and budget charts remain, on the `v` toggle
- `aid retry` resumes a task whose worktree was pruned by auto-GC: it recreates the worktree at the branch tip instead of refusing, so an agent's own committed work is no longer uncontinuable. The guard that prevents orphaning real commits is unchanged
- Tests no longer mutate process-wide state. Seven dispatched runs reported unexplained environment failures that never reproduced in a normal shell; the cause was our own tests setting `HOME` and credential variables for the whole process, so parallel isolation tests resolved inconsistent homes. HOME, credentials and GitButler detection now use thread-local test seams
- End-to-end tests no longer inherit the developer's repository configuration from the working directory: nine e2e files set a temporary AID_HOME but ran with cwd at the repo root, so every dispatched task inside a test picked up this repo's own verify command
- The first-token budget can now be set from the CLI, per agent, or in project config
- `docs/kept-branches.md` records why five branches are kept unmerged and why five others were dropped, including one that had already fixed a bug we re-diagnosed from scratch


## v10.23.0 (2026-08-12)
- Build artifacts now belong to the task that created them: sandboxed agents write the configured shared target dir instead of silently copying it into system temp, and aid reclaims what a finished task owns rather than guessing ownership from directory names
- Sandboxed agents can write the configured `CARGO_TARGET_DIR`: codex receives the effective target leaf as a writable root, and aid creates that leaf when seeding is skipped, so builds land in the shared warm cache instead of falling back to a per-worktree copy under the system temp dir that nothing ever reclaimed
- A dispatched task no longer poisons host build tooling: `CARGO_HOME` and `RUSTUP_HOME` now point at the real host paths while `HOME` stays isolated, so a persistent sccache server can no longer record a task-scoped rustc path that dies with the task and breaks every later build on the machine
- `aid clean --worktrees` reclaims what a terminal task owns — its branch target dir, its sandbox fallback target dir, and its isolated task home — driven by task records. A directory that cannot be attributed to a terminal task is never deleted, and a task whose worktree is still live is never in scope
- Reclamation reports before it removes: every category prints measured sizes and a total, counting each inode once so hardlinked seed artifacts are not double counted
- `aid hook session-start` prints a one-line reclaimable-space hint above a 500 MB threshold, bounded to keep session start under a second. A truncated scan says so and points at `aid clean --worktrees --dry-run` for the exact figure, instead of dropping silently or reporting the threshold as if it were the total
- Cleanup can never remove a cargo target root: branch targets always resolve under the root, and a safety invariant rejects the root itself at every removal site
- `aid merge --force` records the real verification status on the task instead of a stale value read before verification ran, and the override stays visible in `aid show`
- Rate-limit hold tests are clock-independent, replacing hardcoded dates that had begun failing permanently


## v10.22.0 (2026-08-10)
- Task completion now follows process facts, structured agent protocol events, and explicit caller contracts instead of response length, prose shape, or prompt-wording guesses
- Codex exact answers such as `ok` remain successful when a non-empty final message follows the last work event, with end-to-end regression coverage for short delivery
- Verification uses command exit status directly, treats execution failures and timeouts as inconclusive infrastructure outcomes, and no longer reclassifies failures from output keywords
- Explicit result files remain enforceable artifacts, while auto-generated report paths, hollow-output observations, and ordinary model prose cannot silently fail a completed task
- Agent result parsing, quota handling, judge verdicts, checklist responses, model health, skill injection, and smart routing now use explicit contracts without invented defaults or prompt-length thresholds


## v10.21.0 (2026-08-10)
- `aid steer` no longer kills the task it steers: a PTY write that fails is recorded as an undelivered message instead of ending the run, and the agent's own exit decides the outcome again
- Steering, replying, responding and unsticking now refuse up front for agents that cannot read stdin, rather than queueing a message nothing will ever consume
- `accepts_interactive_input()` is a required agent capability, so a new agent must answer the question instead of inheriting a wrong default; `accepts_idle_nudge()` derives from it, and custom agents declare `interactive_input` in their TOML
- Codex dispatch selects its approval flag from the installed CLI version (`--full-auto` before 0.147.0, `--approve-for-me` from 0.147.0) and validates it during preflight
- An unreadable Codex version or an unreachable `codex exec --help` no longer blocks dispatch; aid warns and proceeds rather than treating an unknown as a contract violation
- Task summaries and judges diff against the task's own baseline instead of the previous repository commit, so a no-op task stops borrowing someone else's changes
- A cascade with no recorded worktree, branch or repository keeps the caller's directory instead of aborting the run
- Preflight resolves the agent binary and its capabilities through injectable seams, which took the binary test suite from ten CI failures to zero


## v10.20.0 (2026-08-10)
- A task's judgment is now separate from its lifecycle. `TaskStatus` answers what stage a task is in and whether it was integrated; `VerifyStatus` answers what verification said; a derived `TaskOutcome` answers whether the work succeeded, and it is the only thing any consumer asks. `Verified` and `Delivered` are the only successes — `Unverified(TimedOut | Infrastructure | NoResult)` is inconclusive and `Broken` is a verification failure. One exhaustive derivation, a 120-cell literal golden table over the whole `TaskStatus × VerifyStatus × verify-required` product, and an allowlist wherever success is derived, so a new variant on either axis is a compile error rather than an inherited default.
- Delivered work whose verification failed is no longer counted as a success. Measured on the live store, 378 rows were `done`/`merged` with a failed verification and 87 more had a verify command configured and no result recorded; every counter, gate, chart and webhook read all of them as success. Agent success rates move accordingly — codex 75% to 69%, agy 68% to 60%, mimocode 46% to 38%, commandcode 27% to 9% over 30 days — and `aid advise` routes on those numbers.
- Verification that could not answer is no longer blamed on the change. `VerifyStatus::InfrastructureFailure` classifies a verify run that died on tooling rather than on the code, which is what marked eight tasks FAILED on 2026-08-08 when sccache died after their agents had run the suite green. Tooling noise alongside a real compiler or test diagnostic still fails the task; container-only markers stay gated on containerized runs, and a configured verify command that does not exist stays a loud failure.
- `aid merge` refuses a task whose verification failed or was inconclusive, for single, group and GitButler lane merges alike. `--force` is the explicit override and records why. The group path previously checked only the task status and did not even warn.
- `aid watch --wait` returns a non-zero exit when a task did not succeed, instead of exiting 0 for every terminal state, and it keeps waiting while verification is still running rather than settling on a task mid-verify. A configured verify command now records `Pending` from dispatch, so there is no window in which a verifying task looks like one with no result.
- Exit codes became an allowlist: only outcomes that are success exit 0, rather than everything except an explicit failure. A `Done` task with a timed-out verification used to exit 0.
- Board rows, `aid show`, the task detail and the TUI carry a verification tag only when verification has something to say — `VFAIL`, `VTIMEOUT`, `VINFRA`, `VNORESULT` — decided in one place. A running task and a task that never had a verify command carry none.
- MCP task views, task hook payloads and webhooks gained `outcome` and `verify_status`. Existing fields keep their meaning; consumers deciding whether work succeeded should read `outcome`.
- `aid batch --wait` prints its summary and archives the batch before reporting failure, and a failed serial retry no longer abandons the remaining retries. Failure is an exit code, not an early abort — the moment a task fails is when the summary naming it matters most.
- Codex session resume survives across runs: the durable Codex home is isolated from wrapped runs, background runs stay fresh, a missing session rollout is caught in preflight, and the resume fallback is recorded. Resume matching and model attribution matching are separated, so attribution's tolerance no longer loosens resume's strictness.
- The delivery guard stops false-failing short commit follow-ups. The length floor is waived when the task actually produced a diff, keyed off `HEAD`/`start_sha` and a dirty tree rather than `--read-only`, so audit runs that omit the flag still owe a substantive final message.
- A declared budget is a preference, not a dispatch gate: one shared budget-to-model rule keeps unknown-tier catalogs such as grok selectable as a last resort, and no preferred tier matching warns instead of refusing. Free and Cheap tiers are pooled and priced lowest-first, so opencode no longer prefers a paid cheap model over a free one in an allowed tier.
- `default-skills/aid-guide` documents all of the above.


## v10.19.0 (2026-08-09)
- A held route that the provider itself reports as available is no longer held. aid's quota knowledge was entirely after-the-fact text parsing of a refusal message, so `~/.aid/rate-limit-codex` kept codex out for two days on a stated `Aug 11th, 2026 2:23 PM` while the account had used 0% of its weekly window. aidbar has been probing that number the whole time and aid read none of it. Dispatch now consults the aidbar snapshot cache, and releases a marker only when the snapshot is newer than the marker file and every reported window has headroom.
- A percentage never releases a hold only a person can end. opencode refused at $19.37 of a $20 window, so that window's own 100% is not where the wall is: a `hold: manual` marker, or a refusal whose text requires rereading, stays held no matter what the snapshot says. Absence of evidence is not availability either — a missing probe, an aidbar error, a stale snapshot, and an unmapped provider all leave the marker in force.
- Buffered agents are no longer reaped while they are working. grok and agy write nothing to the PTY until they exit, and the idle watchdog measured PTY bytes alone: all five of grok's silent `exit_code IS NULL` failures since v10.10.0 were aid killing a live process with tools mid-flight, and an agy run died the same way with a 51KB log still growing at the kill instant. `cad6e02d` had taught the first-token detector and the orphan reaper to check agent-log growth; the idle path and the warn/nudge ladder now ask the same question through the same helper. The ladder does not touch the reaper's clock, so a buffered agent that logs steadily without progressing still dies on time.
- codex runs record which model actually ran. All 1257 codex tasks in 30 days carried `observed_model: null` because the `codex exec --json` stream aid captures has no model field. It is in codex's own rollout file, and aid already held the join key: the stream's `thread_id` is the tail of `sessions/<Y>/<M>/<D>/rollout-<ts>-<thread_id>.jsonl`, whose `turn_context` line names the model. Unknown still stays null — no rollout, no `turn_context`, or no model key yields null rather than the model that was requested.
- Investigation and audit records for the above land in `docs/`.


## v10.18.0 (2026-08-09)
- A route aid already knew was held is no longer dispatched. `aid run codex` against a marker stating a three-day outage used to spawn codex anyway, collect the refusal four seconds later, record a FAIL, and only then cascade — nine times in one day against a hold recorded at 02:39. Substitution now happens before dispatch: no task row for the agent never run, the warning and a task event both name `aid config clear-limit <agent>`, and `--declared-urgency background` still keeps the agent you asked for.
- A substituted route carries none of the held route's context. `switch_agent` drops both the model and the session id when the agent changes, and every site that renames an agent — cascade, `aid retry --agent`, batch retry, and each racer in `--best-of` — now goes through it instead of re-deriving the rule. Previously `--best-of` handed every competing CLI the same session token.
- Quota state is visible where the agent is chosen. `aid agent list` gains a STATUS column, shown only when something is actually held. A model-group hold reads as PARTIAL rather than as the agent being down, so cursor with its premium pool exhausted is still dispatchable on `auto`. `aid agent quota`, `aid agent list` and the agent JSON API now agree, and custom agents appear in all three.
- aid no longer invents a reset time it never observed. A hold that only a person can end reads as `held until cleared with aid config clear-limit <agent>` instead of the fabricated `resets ~1h`, and the command it prints resolves through the same function that decides the marker file — so the hint can no longer name a different agent than the one held.
- Each custom agent gets its own rate-limit marker. Every custom agent previously shared `~/.aid/rate-limit-custom`, so one hitting its quota held all the others down and clearing one cleared them all. Built-in marker paths are unchanged, and the reaper's blanket skip of custom agents — a workaround for this bug — is gone.
- A buffered agent survives the first-token budget on the strength of the log it writes. grok emits nothing to the terminal until it exits, so every run past 180 seconds was reaped as hung; the same rule already existed in the orphan supervisor and had never reached the live watcher. A run that writes no bytes anywhere is still killed on time. The idle budget is not yet covered by this: it still counts progress events only, so a buffered agent quiet for the full idle window (default 600s) is still reaped — raise it with `--idle-timeout` until that is fixed.
- An unresolvable `--cascade` entry is an error rather than a silently skipped one, and custom agents are valid cascade targets in both `aid run` and `aid batch`.


## v10.17.2 (2026-08-08)
- aid was killing healthy agy runs. It invokes `agy -p <prompt> --print-timeout 24h`, and print mode emits nothing on stdout until a turn completes — so agy's time-to-first-byte is its first-turn latency, and aid reaped "zero progress since spawn" on the 180s first-token budget. The two settings contradicted each other: aid granted agy 24 hours and killed it after three minutes. Any agy task whose first turn ran past 180s died, deterministically.
- aid now hands agy a per-task log path and the orphan reaper watches that file alongside the transcript, so an agent that is streaming from the model instead of writing to stdout reads as alive. Replaying the run that prompted this: its log last grew 150s before the kill, inside the budget.
- Watched bytes only count when written at or after the task started. An `agent.log` left by an earlier attempt on the same task id used to count as progress by merely existing, quietly moving a spawn that produced nothing off the first-token budget.
- The log path is handed out only where the wrapping is known — never for sandboxed or containerised runs, whose guests cannot write a host path — so no agent adapter has to reason about isolation.
- Known limit, stated in the code rather than papered over: a process looping forever while appending looks identical to one doing work. No byte-level signal separates them; max-duration and the hard cap remain the bound on such a run.


## v10.17.1 (2026-08-08)
- A task that committed cleanly could still be recorded FAIL with "Configured verification did not run: dirty worktree rescue dispatched retry before verify". The dirt was aid's own: an agent had run `git add .aid-lock`, so aid's lease file became tracked, and aid clearing that lease at task end showed up as ` D .aid-lock` — which the dirty gate read as the agent leaving work behind. Rescue already ignored the file; the gate downstream did not. Both now ask the same question.
- Creating a worktree adds `.aid-*`, `aid-batch-*` and `result-t-*` to the repository's local `.git/info/exclude`, so aid's runtime files never become tracked in the first place. The file is never committed and the repository's `.gitignore` is left alone. (A per-worktree `info/exclude` does not work — git does not read it — so the entries go to the common directory and apply to the whole clone.)
- A porcelain rename names two paths on one line. Judging such a line by its destination alone would have let `R src/lib.rs -> result-t-abcd.md` erase a real file from the retry gate, the data-loss assertion, the reaper and stop; a line now stops counting only when every path on it is aid's.
- The aid-owned test is narrower than the `git add` exclusion list on purpose: over-excluding a `git add` only leaves a file uncommitted, while over-matching here makes real work stop counting as uncommitted at all. A user's own `result-summary.md` is theirs; only `result-t-*` is aid's.


## v10.17.0 (2026-08-08)
- A dashboard was delivered and the reviewer concluded nothing had been built. Both halves of that were aid's doing, and both are fixed here.
- The dirty-worktree rescue ran `git commit --amend` whenever a HEAD existed, so it folded an agent's work into whatever commit happened to be there — 14,303 lines of a delivered dashboard ended up inside the operator's own commit titled "chore: ignore aid's worktree bookkeeping files", and `git log` then showed no trace of the work. Rescue now amends only a commit the task itself created: `start_sha` known and different from HEAD. Unknown, equal, or tagged means a new commit.
- The follow-up task aid dispatched to commit the leftovers was scoped, correctly, to `start_sha..HEAD` — a two-line .gitignore — and that sliver was all `aid show --diff` showed. The task-scoped default is right and stays; what was missing was any sign the branch held more. The diff stat now says so whenever commits sit below the task's baseline.
- New `aid show <task-id> --diff --branch` widens the view to every change since the branch left the default branch. Read it before concluding a task produced nothing.
- `aid show --json` now emits `start_sha`, `final_head_sha` and `final_branch`, which it had been omitting entirely — the fields needed to tell a task's own scope from its branch's.
- The branch base is resolved to a ref that actually exists, keeping the `origin/` prefix `merge-base` needs in a clone with no local default branch, instead of falling back to the literal name "main". When no base resolves, `--branch` says the branch view is unavailable rather than labelling an empty diff "whole branch".


## v10.16.0 (2026-08-07)
- A quota refusal is now attributed to the CLI that spoke it, not to any text that happened to contain a needle. Four false holds were written on this machine in one afternoon, each from something the model wrote or a tool printed: a cursor audit quoting a test fixture, a row of that audit's own markdown table, its grep command line, and a claude agent's failed-edit error — that last one held claude out of rotation for a day while it was editing the very test meant to prevent this. Detection reads the part of a run's output the CLI itself produced; what the model writes arrives inside a field the CLI opened and can no longer promote itself into testimony about the provider.
- The rule states what it cannot catch instead of implying it catches everything: an agent whose CLI prints its refusal as bare stdout alongside the model's answer has no structural split, qwen reports an exhausted plan in the same slot its model fills, and a wording nobody has captured stays undetectable.
- A verification that ran out of time is no longer recorded as one that failed. `VerifyStatus::TimedOut` is its own outcome: the wall-clock cap says nothing about whether the change under test is broken, and five parallel audits queuing on one cargo build lock were being reported as five failed tasks.
- A read-only task is no longer verified. It changes nothing, so the project's default verify command had nothing to exercise and cost a full build per task. An empty diff is deliberately *not* a skip — "the agent delivered nothing" is a delivery fact, and the configured verify must still run against the tree.


## v10.15.0 (2026-08-07)
- A quota refusal is now held by what actually ends it, not by a guessed number of minutes. Three classes replace the single cooldown: a stated reset time, a hold only a person can end (a spent balance, a monthly cap), and a bounded cooldown for a refusal that names nothing. Before this, a message with no stated reset fell back to a five-minute window, so the least recoverable refusals — grok's spent balance, copilot's monthly quota — expired fastest, and aid resumed dispatching into an account that could not serve.
- Cursor's premium pool and its `auto` tier are metered separately. Cursor's live refusal ("You're out of usage. Switch to Auto") matched no signature at all, so three tasks died against a route aid still reported as healthy; it now marks the tier the refusal names and leaves `auto` dispatchable. Copilot's structured `quota_exceeded` and grok's `usage balance exhausted` are recognized for the first time.
- `aid run` no longer walks into a route it knows is out, and no longer diverts away from one that has recovered. The dispatch gate keyed on a marker carrying a recovery time, which both let human-ended holds through and kept diverting for a time that had already passed.
- `aid config agents` reports the state it actually computes. An expired marker was rendered as "rate-limited" from its stale text, and grok — absent from the catalog entirely — never appeared at all, which also meant `aid config clear-limit all` skipped it and `aid advise` could never recommend it.
- A dispatch that cannot run is refused before the task row exists. `aid run` accepted a task, returned, and only then died in the background worker on an unsupported flag combination or a missing agent binary; the caller believed a task was running.
- A task whose agent process never started now ends in a terminal state. t-0132d608 recorded its spawn failure and stayed `running` for hours: phantom work on the board, a `watch --wait` that never returns, and a failure counted as neither success nor failure.
- grok is a usable route again. Our own global Stop hook told every headless grok run to ask the boss first, and `hiboss ask` blocks for a human who is not there — so grok wrote zero bytes and was reaped twenty minutes later, reported as an agent hang. Its dispatch now refuses that gate by policy.
- A buffered agent that has produced nothing since spawn is reaped on the first-token budget instead of twice the idle timeout, keyed on bytes observed rather than events counted — buffered agents emit no incremental events at all, so counting them killed healthy runs and spared silent ones.
- "Missing final delivery" no longer overwrites the real cause of a failure. A run refused for quota in seven seconds, or one that never started, was reported as having failed to deliver a report. The delivery assessment is still recorded; only the diagnosis defers to the terminal cause that was already known.
- `aid --version` carries the build's git describe and dirty flag, and says so plainly when there is no git metadata rather than looking like a clean release build. Eight times in one day a fix was verified against a binary from another branch.
- aid stops committing its own bookkeeping into user branches. `.aid-lock` and `.aid-verify-deps-state` were being staged by three separate `git add` sites; the exclusions are now shared, cover nested paths, and deliberately do not exclude files a repo legitimately tracks under `.aid/`.


## v10.14.0 (2026-08-07)
- An agent on the buffered path had its quota refusals ignored entirely. `record_quota_exhaustion` — the anchored-signature machinery v10.12.0 was built around — ran only from the streaming and PTY watchers, and `watch_buffered` called neither. Two built-ins take that path (`agy` and `grok`, both declaring `streaming() = false` and `needs_pty() = false`), so for them a real outage was detected only by the coarser stderr and log scan that the anchored rule exists to replace. The buffered watcher now runs the same detection with the same three outcomes and the same clear-on-success rule. This was found by a cross-audit asking why the fix in front of it targeted a state that could not occur.
- A per-model-family rate-limit marker is cleared by a success on that family and by nothing else. For a provider that meters families separately, exhaustion writes `rate-limit-<agent>--<family>` while every success path cleared only `rate-limit-<agent>`, so the family stayed dead until someone deleted the file by hand. Clearing every marker at once is still available and is now reachable only from `aid config clear-limit`, the one caller that means it. Both halves were unreachable in production until the buffered path above began writing those markers at all.
- A quota refusal is no longer missed because the scan window opened in the middle of it. The window sliced the last bytes of output at a raw offset, cutting a refusal line in half so its anchored signature stopped matching. It now begins at a line start and spans 64 KB rather than 4 KB, because an agent that prints more than 4 KB of diagnostics after a refusal pushed the refusal out of the window entirely. The rewind to a line start is itself bounded to one extra window, so output containing no earlier newline cannot turn every task completion into a full-buffer scan. An earlier attempt at this alignment moved *forward* to the next newline instead, which dropped a whole line and was a regression for any refusal longer than the slack; both of its tests built input under the threshold and never reached the branch they claimed to cover.
- A task that produced a report is no longer recorded as having produced nothing. The message extractor dispatched on the top-level `type` field, so a CLI that wraps its stream as `{"type":"event","event":{…}}` matched no case; the fallback then kept only lines that fail to parse as JSON, which for an all-JSON stream leaves aid's own terminal sentinel as the entire output — 42 bytes. Measured on two real cross-audits: one ran 52 minutes and its report had to be recovered by hand from a 155 MB transcript, the next was recorded FAILED by the delivery guard while its transcript held the finished report.
- When no arm matches at all, aid says so instead of writing the sentinel. The notice names the agent — taken from the task record, never inferred from a field in the stream — quotes a sample line, and points at the transcript, and the delivery guard reads it as evidence of work rather than its absence. A file containing nothing but that sentinel is no longer returned as a task's output.
- Streaming text deltas are assembled into whole messages and thinking deltas are dropped, which `cursor` already did and `copilot` had a third copy of. A residual case is filed rather than papered over: when aid recognises part of a stream but not the envelope the deliverable arrives in, the report is still lost silently, and the share-based heuristic that would catch it needs a threshold measured against real logs first.


## v10.13.0 (2026-08-06)
- A dispatched agent no longer reads the orchestrator's own agent-instruction files. A CLI discovers `~/.claude/CLAUDE.md`, `~/.claude/settings.json` and the user-scope skill and sub-agent definitions from `$HOME`, so every agent aid dispatched was handed this machine's operating instructions — including the section telling it to delegate work, which one agent followed by dispatching a sub-agent of its own and then blocking for ten minutes waiting on it, and a notification protocol another used to ask the operator a question and wait for an answer. aid injects none of this; the CLI finds it, so the fix is at dispatch time. Each task now runs with its own `$HOME`: a directory of symlinks to the real home with the instruction surfaces removed. Measured with `grok inspect` in an empty directory — 71 user-scope skills before, 21 after, which is the bundled set that ships with the CLI.
- Where a directory mixes the orchestrator's instructions with a CLI's credentials, the masking is per entry rather than per directory. Denying `~/.claude` wholesale broke `aid run claude` outright, because that is also where the Claude Code CLI keeps its own credentials — a cross-audit caught it before release. `.claude` is now materialised as a real directory whose children are symlinked except the instruction-bearing ones: `CLAUDE.md`, `settings.json`, `settings.local.json`, `.mcp.json`, `skills`, `agents`, `commands`, `plugins`, `hooks`, `tools`, `rules`, `workflows`, `memory` and `plans`. Verified by running rather than by reading: `claude -p` exits 1 with "Not logged in" under the wholesale denylist and exits 0 under per-entry masking.
- Isolation fails closed. Every step of building the isolated home — removing a stale one, creating the directory, reading the real home, each symlink — now propagates its error with the offending path in it, and no `$HOME` is exported unless the directory was built as intended. When `HOME` is unset the real home is resolved from the passwd entry rather than substituted with an empty directory, which is what an orchestrator started by launchd or cron would otherwise have handed every agent.
- A containerised run never receives a host path as `$HOME`. `exec_in_container` forwarded every environment variable verbatim, so the isolated home — a path that exists only on the host and is never mounted — arrived as the container's `$HOME` and every `$HOME`-relative write failed. The wrapper now drops any forwarded `HOME` and sets `HOME=/root`, where the agent's configuration is actually mounted. Dropping it before setting it means the fix does not depend on which of two duplicate `-e` flags a container runtime happens to honour, a question nobody had answered.
- The rule that a refusal in text the agent itself wrote counts only against that provider's own anchored template now applies everywhere such text is scanned. v10.12.0 established the rule for the completion buffer and missed two paths: every parsed streaming event, and the fallback that scans the whole task log when stderr says nothing. It was caught live — a task was marked rate-limited because its own tool-call line read `completed: grep clear_rate_limit_if_stale|marker_path`, which locked that provider out of routing in the middle of a session. Structured error envelopes and stderr keep their existing sensitivity; those are channels the agent does not author, and each of the six refusal shapes this repo has captured still reaches a marker.
- A unit-test run can no longer mutate the developer's live `~/.aid`. A `cfg(test)` assertion refuses to resolve a rate-limit marker path against the real home when no test-local home is active, and it exposed nine further test files reaching outside their fixtures, all now isolated. Its limits are documented where it is defined: it catches same-thread calls through `marker_path`, and it catches neither a call on a spawned thread — a panic there fails no test — nor any access that does not go through that function. The vanishing-marker report that prompted it is therefore still open, and the guard does not claim to close it.


## v10.12.0 (2026-08-06)
- A self-hosted model endpoint is now a route aid can describe. A BYOK agent pointing at Ollama on a LAN box reported `provider = unknown` and `egress = third-party` — the same tier as OpenAI — and inherited opencode's rate-limit marker, so the one route that cannot be exhausted went dead whenever opencode Zen ran out of balance.
- `EgressTier` gains a private-network tier for RFC1918, link-local and IPv6 ULA endpoints and for mDNS suffixes. `--egress local` is unchanged and still admits loopback only: widening an existing safety flag would have silently sent prompts to a LAN host for callers who asked for same-machine. The new tier has its own opt-in, `--egress private-network`. Host classification parses the host as an IP first and falls back to a DNS suffix, never a leading-character test — an earlier round of this work classified `fc2.com` as the operator's own network.
- Provider identity for a custom agent is declared in the BYOK manifest and carried into the generated agent TOML, never inferred from a hostname. An absent declaration stays `unknown`.
- A custom agent that declares its own `base_url` no longer inherits opencode's rate-limit marker.
- `aid byok probe` no longer reports a capable model as unable to call tools. It ran thinking models on a 128-token budget, which they spent before emitting a call. Measured against Ollama qwen3:4b: 128 tokens gives `finish_reason: length` and zero tool calls, 2048 gives a correct call. `length` is now its own inconclusive outcome (exit 3) rather than a negative.
- aid no longer marks a successful task FAILED because the agent wrote about rate limits. Quota detection matched bare substrings — `rate_limit`, `credits`, `429`, `rate limit` — anywhere in the agent's own output. On 2026-08-06 that failed two successful tasks and locked cursor out of routing twice: once on an audit report's paragraph about Aave price oracles, once on the fixing agent's own sentence listing which refusal templates it had preserved. Two earlier fixes narrowed where the scan looked; a report's conclusions live in its tail, so tail-only scanning made reports more exposed, not less.
- A refusal in assistant-authored text is now recognised only by its provider's own anchored template. Generic signals — 429, 402, `rate limit`, `too many requests` — count only on a channel the agent does not author: a structured error event, stderr, or an HTTP status line. Providers whose refusal wording has never been captured (claude, grok, commandcode, custom) are undetectable on the prose channel and are reported as such rather than guessed; all agents remain covered on the non-agent channels.
- Detecting an outage, marking the provider, and failing the task are now three separate decisions. A run that delivered a report and also hit a refusal keeps its Done status while the provider stays marked, so routing avoids it.
- A rate-limit marker written during a run now survives that run's success. `handle_done_postprocess` cleared the marker on every completed task, running after the watcher and erasing an outage microseconds after it was recorded. Only a marker that predates the run is stale.


## v10.11.0 (2026-08-06)
- A custom agent now declares a route aid cannot otherwise reach — a different CLI, or a different provider and model behind a wrapper — and nothing else. Eleven of the seventeen definitions on this machine were personas over a route aid already had: they ran droid, codex or agy with nothing changed but a strengths list and a capability table. A fork like that inherits none of the adapter it copies. Measured on droid: the built-in sends `exec --output-format stream-json --skip-permissions-unsafe`, while four forks sent the bare binary, which opens droid's interactive TUI and asks "Trust this folder?" on a worktree path that is new on every dispatch — so droid's per-path trust store can never hit and the task idles at the prompt until it is reaped. Forks also report `provider = unknown` while spending the built-in's quota, and score on a second capability scale that cannot be compared with the first.
- The agent registry refuses a definition whose command is a built-in's binary, and the error names the two supported replacements: `--skill` for a persona on a real route, `delegate_to` with `forced_model` for a different model on a built-in CLI. `built_in_binaries()` is now the single source of truth that both the PATH preflight and this guard read, so a new agent cannot be reachable by one and invisible to the other.
- Codebuff support is removed entirely: the adapter, the enum variant, the capability and pricing rows, the sandbox and container wiring, the Node plugin, and the documentation. Historical references in the changelog and in two incident comments are left as the record they are.


## v10.10.0 (2026-08-06)
- `aid build` no longer reports a run that compiled nothing as success. `finished: 0 errors, 0 warnings, 0 units compiled` reads as a clean build and is what actually misled an agent into believing its tests had run; raw cargo exits 101 loudly for the same invocation, so the silence was the wrapper's. An invocation that matched no targets now fails and says so, while a genuinely cached no-op build — everything fresh, cargo exits 0 — is still reported as the success it is. Cargo's own exit code is preserved either way.
- Working agents are no longer reaped as hung. The idle clock advanced only on output aid could parse into an event, so for a CLI whose output it cannot parse the clock never advanced at all and the reap was guaranteed for any task over 600 seconds regardless of real activity: one agent wrote 448 lines across 11 files while aid captured 87 bytes and killed it. Raw output that carries text now counts as progress; pure terminal-control noise, aid's own idle nudges and their PTY echoes still do not, so a silent or wedged process is still reaped.
- `aid retry` on a STALLED task now supersedes that task's own run instead of being refused by its own worktree lease. The old error named the stalled task as the conflicting holder and told the caller to "use separate worktree names for parallel tasks", sending them looking for a second task that does not exist. The retry stops the prior worker first and refuses only if it cannot actually be stopped — it never enters a worktree that still has a live process, and a genuinely rival task is still refused.
- A `--worktree` dispatch no longer warns that a code agent without `--dir` may be unable to write files. The check looked only at `--dir` and never at `--worktree`, so the dispatch form the guide tells callers to prefer always was the one aid complained about.
- This repository's own `.aid/project.toml` is valid again. It carried `aid_gc = "auto"`, a setting aid removed and deliberately rejects, and because the config denies unknown fields the whole file failed to deserialize — leaving profile, team, verify command, budget, six rules and a fourteen-entry knowledge index silently unused on every dispatch from this repo. It now also declares `skills = ["implementer"]`, which since v10.5.0 is the only way a project gets a default skill.
- Three tests no longer read the developer's own `.aid/project.toml`. They passed for months only because this repo's project file was the broken one above; `effective_skills` grew a seam that takes the project default as a parameter, and two batch fixtures now declare the task profile they always needed.


## v10.9.0 (2026-08-06)
- Added: `aid test` — a test run whose result can be trusted. A filter matching zero tests is now an error naming the filter (`cargo test -- <typo>` runs nothing and reports `ok` with exit 0); a run that produced no test targets fails instead of looking fine; the tests that actually ran are listed rather than only counted, so you can confirm the ones you just wrote were among them; and failure output is compacted to the panics and assertion diffs. `--test` selects an integration-test target and says so — it is not a name filter, and conflating the two is what made the first attempt pass its unit tests while failing on the first real invocation. `--isolated` gives the cargo child a temporary `AID_HOME` so a test run cannot read or pollute `~/.aid/`.
- Fixed: the TUI selection jumped to a different task whenever a new one appeared. Selection was a positional index and a refresh replaced the whole list with only a length clamp, so a newly created task took index 0, everything shifted down, and the highlight — plus the events pane below it — silently moved to another task. Selection now follows the task's identity across a refresh, and falls back to a nearby index only when the selected task is genuinely gone. `tree_selected` had the same defect and now anchors on its node identity.
- Fixed: a finished task rendered grey in the default TUI view. `ui_helpers::status_style` painted Done/Merged as grey while the identically-named `dashboard::status_style` painted it green, so which colour a completed task got depended on which view you were in — and `aid watch --tui` opens the grey one. Success is green in every view now. The duplication that allowed the two to diverge is filed as wi-71cf.
- Fixed: detail scroll offsets could run past the end of the content, and completed rows were dimmed harder than running ones, which made a successful task read as spent.


## v10.8.0 (2026-08-06)
- Fixed: every `grok` task was recorded as failed, with its model, cost and token count discarded. `finalize_buffered` appends aid's terminal sentinel to the captured buffer before `parse_completion` reads it, so requiring the whole buffer to be one JSON document failed on every run — an echoed idle nudge was a second contaminant. The stopReason guard below it never ran in the cases it was written for. grok's recorded success rate, its `aid stats` figures, its `aid advise` history score and every cascade decision involving it were reading a constant.
- Fixed: a cascade to a different agent carried the old CLI's model with it. codex's `gpt-5.6-sol` reached agy, which refuses it by listing its own Gemini models. v10.5.1 shipped as "dropped in all three switch paths" but covered only the hung-retry cascade, batch retry and the model self-heal — it missed four sites including both cascades in `run_lifecycle`, which is the path a quota failure actually takes. The rule now lives in one `switch_agent` function that all five sites call.
- Fixed: an unknown model's cost was recorded as `$0.00` rather than as unknown. aid now reads a normalised price feed (428 canonical models) with the built-in matcher as its offline fallback, and distinguishes four states — priced, included in a flat-rate subscription, matched offline, and unknown. No path renders an unknown model as `$0.00` or "free". The feed is refreshed out of band; the dispatch path never touches the network.
- Fixed: an agent name this binary cannot parse became `custom` with the real name discarded, so the board showed `custom/unknown/unknown` and which agent had run was unknowable. The unparsed string is now kept; a genuine custom agent's configured name still wins.
- Added: `--model` is validated against the target CLI's own served list (grok, codex, agy, cursor, qwen) rather than aid's catalogue. Unknown means allowed — a CLI that cannot be queried never blocks a dispatch — and `cursor --model auto` keeps working.
- Added: `--egress local|any` declares data egress separately from `--rigor critical`, which no longer restricts which route may run. Egress is decided by the provider, not by a per-CLI constant: `codex` was labelled "local" while its provider is `openai-chatgpt-plan`, and on one day codex, oz and codebuff were all unavailable at once, stranding critical work for a reason unrelated to trust.
- Added: Command Code (`commandcode`) as a built-in agent — one billing entity reselling 52 models across Anthropic, OpenAI, Google, xAI, Meta, DeepSeek, Moonshot and Z-AI. Write runs pass `--yolo`, read-only runs `--permission-mode plan`, and its `model_request_start` event gives real model attribution.
- Fixed: two unit tests read the developer's live `~/.aid/` — the critical-rigor advise test asserted against whatever quota markers the machine happened to carry, and the pricing tests against a real cached price feed. Both now run under an isolated `AID_HOME`.


## v10.7.0 (2026-08-06)
- Fixed: `grok` write runs cancelled their own edits and still billed. aid passed no approval flag, and in headless `-p` mode grok renders no prompt — it abandons the tool call, reports `stopReason: "cancelled"` and exits 0. Measured: a one-line file edit left the file untouched and charged $0.045; a 32-turn dispatch burned $1.04 and delivered nothing. `--permission-mode auto` and `dontAsk` behave identically; write runs now pass `--always-approve`, and read-only runs keep `--permission-mode plan` and are asserted never to carry blanket approval.
- Fixed: a quota failure that produced nothing was recorded as Done and suppressed the cascade. `rescue_quota_failed_task` rescued on "verify passed" alone, but on an untouched worktree `cargo check` succeeds precisely because nothing changed — one oz task was stored `status=done` with `exit_code=1` and its `--cascade` never ran. The guard now asks whether the agent produced anything, using worktree-local signals only: untracked files, HEAD versus the SHA recorded at dispatch, and `git diff HEAD`/`--cached`. Event count is deliberately not used — it is not evidence of work in either direction.
- Added: `aid`'s task views name the execution route as the three things it is — CLI, provider, model — instead of one opaque agent name, and mark how the model was attributed. An unknown model stays the literal "unknown"; a model aid asked for and a model the agent confirmed stay distinguishable.


## v10.6.0 (2026-08-06)
- `aid hook session-start` and the official guide now state what the dispatcher owns rather than repeating a command list available from `--help`. The session-start text is injected into every session, so it is the one place a caller reliably reads.
- It leads with what aid refuses to guess: declare the task profile, declare `--skill` because aid picks none, declare `--kind` to narrow the injected toolbox because omitting it describes every tool. Undeclared is stored as null, not inferred.
- It states the two routing rules learned by breaking them: a route is `<cli>/<provider>/<model>` and one exhausted route says nothing about another provider reaching a model of the same class; and never dispatch to a weaker model on the provider pool the caller is already running on, because a different provider is delegation while the same pool for a worse model is waste.
- It documents a hazard that is easy to misdiagnose. aid snapshots a directory's dirty paths once, at dispatch, and excludes them when rescuing an agent's uncommitted output — so edits made before dispatch are protected, and edits made during the run are not, being indistinguishable from the agent's. The rule is not "commit before dispatching" but "do not edit a directory an agent is working in"; `--worktree` puts the agent somewhere else entirely.


## v10.5.1 (2026-08-06)
- Fixed: `aid run --dry-run` left a phantom failure behind. A dry run builds a real task row to resolve the prompt and then returns without dispatching; left in `pending`, the background reaper found it ten minutes later and recorded "Task timed out in pending state after 602s (reason: unknown)" against an agent that was never invoked. Dry runs now end as `skipped`.
- That was corrupting routing, not just the board. `agent_success_rates` counts `done|merged|failed` and is what `aid advise` weights recommendations with, so every dry run quietly lowered an agent's score. Sixteen phantom rows accumulated in one day; agy's 30-day success rate read 73.7% where excluding only those rows it is 79.7%. `skipped` is excluded from both the reaper and the success-rate queries.
- Fixed: a model outlived its route. A codex task on `gpt-5.6-luna` failed, cascaded to agy, and carried codex's model with it; agy refused by listing its own models. A model name means something only inside one CLI, so it is now dropped wherever a derived dispatch changes agent — the cascade, `aid retry --agent`, and batch retry's rate-limit fallback. A same-agent retry still asks for what was asked before.
- Fixed a test that read the machine's live rate-limit markers and so took a different code path depending on whether codex happened to be exhausted at that moment. It now runs against an isolated home.


## v10.5.0 (2026-08-06)
- BREAKING: aid no longer picks a skill for you. `auto_skills` chose by agent kind alone and never looked at the task — every implementation CLI was handed `implementer`, gemini and agy were handed `researcher`, whatever the work was. Skills now come from `--skill`, then `--no-skill` as an explicit none, then a project default (`skills = ["implementer"]` in `.aid/project.toml`), then nothing. A project that wants the old behaviour declares it once.
- BREAKING: omitting `--kind` now describes every resolved toolbox tool instead of filtering by a guessed category. A multi-file refactor described in one tight sentence classified as `simple_edit` and received 2 of 24 tools with nothing in the output to say what had been dropped. Narrowing is opt-in: declare `--kind` and tools are filtered to that category, because omission is not a decision.
- `aid show --context` and `aid export` report the skills a task was actually dispatched with, read from its stored args. They used to re-derive them from the agent kind, which reported `implementer` for a codex task even when the caller had passed `--skill reviewer`. Tasks dispatched before skills became declared report none, which is what their record says.
- A `--tool` flag for finer selection is deliberately not included yet: it would have to thread through a 45-parameter dispatch chain, and removing the guessing did not need it — `--kind` is the caller's narrowing mechanism and already exists.


## v10.4.0 (2026-08-06)
- Fixed: `aid run --kind <category>` was accepted and then ignored. The classifier only ever saw the prompt text, so the declared kind never reached the task profile and toolbox filtering and skill auto-apply kept using the keyword guess. Declaring the kind is now how a caller stops aid from guessing which tools and skill it gets.
- Caught by using it: dispatching research about aid's own tool selection with `--kind research` still printed "Injected 2/24 toolbox tool(s) (filtered by frontend)". Accepting a declaration and then discarding it is worse than not offering the flag, because the caller believes it decided something.
- Added a regression test pinning the exact codex quota string captured on 2026-08-05, including its ordinal day suffix. If that format ever stops parsing, `is_rate_limited` silently falls back to a 300-second window and a six-day outage reads as available again after five minutes.


## v10.3.0 (2026-08-06)
- An execution route is now three things rather than one opaque agent id: the CLI that is invoked, the provider that meters and bills it, and the model that does the work. `aid advise` names the recommended route as `codex/openai-chatgpt-plan/gpt-5.6-luna`, and `aid agent list --json` carries `provider` and `metering` per agent. Agent names are unchanged — `aid run codex` resolves to a route.
- `metering` says how a provider meters, which decides what one outage implies: `account_pool` (one pool for the whole account), `per_model_family` (one exhausted family says nothing about the others), `spend_budget` (a currency budget that does not refill with time — only a top-up clears it), `subscription`, `none`, and `unknown`.
- Every provider in the table was established from a real refusal captured by this repo: codex's usage-limit page, qwen's ModelStudio token-plan base URL, agy's per-family "Individual quota reached", opencode Zen's HTTP 401 "Insufficient balance", oz's Warp log path, droid's weekly 402. Providers nobody has watched refuse are `unknown`, which is a real answer rather than a gap to fill with a plausible name.
- The change is additive. `AgentKind` is not renamed or deleted — it always was the CLI dimension carrying two extra jobs, and it appears in 203 files, so the two extra jobs were taken away from it instead of rewriting it.
- Retired a hardcoded special case: whether quota is metered per model family was `matches!(agent, AgentKind::Antigravity)`, written the day agy's per-family metering was discovered. It is a fact about the provider, so the provider table answers it now and a second such provider needs no code change.
- Removed a duplicate that had already diverged: model-family classification existed in two places with two different answers for `gpt-*`. One copy now, in the types layer.


## v10.2.0 (2026-08-06)
- A task's observed model now carries an evidence grade. `attribution_source` is `echoed` when the CLI named the model in its own output, and `confirmed_by_success` when aid passed an explicit model and the run succeeded — a CLI handed a model it cannot serve fails instead. It moves with `observed_model` and is null whenever that is.
- The second grade exists because some CLIs never name a model: codex emits 593 KB of JSONL with no model string anywhere, and agy's plain-text output has nothing to read. Without it their tasks stay permanently unknown, which starves the model-level history that routing is built on.
- Consumers take the strength they need. `aid stats` accepts either grade; an agent's learned default model accepts `echoed` only, because a model inferred from a run not failing is not evidence that model performed well.
- A router alias such as `auto` is never confirmed by success. It selects a model rather than being one, and `cursor.model = "auto"` is a real entry in `agent_config.toml`, so confirming it would put a router back in the model column.
- Fixed: models the CLI had already reported were being thrown away. `apply_completion_event` returned early unless the event was a completion, but copilot names its model on a milestone event and droid on its first `system/init` line, and neither repeats it at the end. copilot stored no model for 19 of 20 tasks while `"model":"gpt-5-mini"` sat in its log three times over; droid 14 of 15 with `"model":"claude-opus-5"` on line 0. A model announcement is now taken from any event that carries one, while tokens and cost stay completion-only since a mid-run value is partial.
- `aid show --json`, `aid board --json`, the MCP task view and the web API emit `attribution_source` alongside `requested_model` and `observed_model`.
- Human surfaces distinguish the grades: `gpt-5.6` when the CLI said so, `gpt-5.6 (inferred)` when the model was confirmed by the run succeeding rather than stated, `gpt-5.6?` when a request was never confirmed at all, and `composer-2 (asked auto)` when the CLI served something other than what was asked for.
- Known limit: `parse_event` is consumed only by the non-PTY streaming watcher, so an interactive PTY dispatch has no event stream to learn a model from and still falls back to the adapter's completion parser.


## v10.1.0 (2026-08-06)
- BREAKING: a task now records two models instead of one. `requested_model` is what aid dispatched with; `observed_model` is what the CLI reported it actually ran, and stays null when the CLI reported nothing. `aid show --json`, `aid board --json`, the MCP task view and the web API emit both fields and no longer emit a single `model`.
- Fixed: aid stored a dispatched model as if the CLI had confirmed it. Three copies of `info.model.as_deref().or(model)` let a request masquerade as an observation, which recorded the `claude` CLI running `gemini-3.6-flash-low` and `agy` running cursor's `composer-2` — both failed runs where the CLI had refused the model — and recorded `auto`, a router, as a model.
- `aid stats`'s per-model breakdown and an agent's learned default model now read the observation only, so an unconfirmed model reads as `unknown` instead of being reported as fact. Several CLIs never report a model, so `unknown` is the expected value for many tasks. Cost estimation still falls back to the request, and both fields are stored so a reader can tell which basis a row used.
- Human surfaces show the pair: `gpt-5.6` when request and observation agree, `gpt-5.6?` when a request was never confirmed, and `composer-2 (asked auto)` when the CLI served something other than what was asked for.
- Fixed: grok runs that stopped early were recorded as successes. grok reports a cut-short run in the same envelope shape as a completed one — no error type, real text, real usage and cost — and only `stopReason` tells them apart. Three stored tasks carry `cancelled`, one of them an audit that stopped mid-sentence after five turns and $0.22.
- Fixed: opencode's insufficient-balance refusal went unrecognised. It arrives as an HTTP 401 body rather than a 429 or 402, so no marker was written, `aid agent quota` kept reporting opencode as OK, and aid kept dispatching to an account that could not pay. Unlike a time-based quota this does not recover on its own, so the cooldown is a day, with `aid config clear-limit opencode` as the escape hatch after topping up.
- Retries, cascades, best-of children and self-heal retries inherit the requested model rather than an incidental observation, so a derived dispatch asks for what was asked before instead of freezing a CLI's default into an explicit request.
- Per-family quota marking continues to read the requested model on purpose: it asks which family aid aimed at, and plain-text CLIs such as agy never echo a model at all.
- Historical task rows are not rewritten. They were written by the collapsing path, so they are requests at best and wrong guesses at worst; back-filling observations from them would launder guesses into evidence.


## v10.0.0 (2026-08-05)
- BREAKING: the `auto` agent is removed. Declare a task profile and use `aid advise` to pick an agent; `aid run auto` and batch `agent = "auto"` now fail with that hint.
- BREAKING: agent success rates recorded before this release are inflated. Streaming agents took their final status from the exit code alone, so a CLI that exited 0 while reporting an API, auth, or quota error was stored as done. Historical rows are not rewritten — treat pre-v10 success rates as an upper bound.
- Tasks now carry a declared profile: `--difficulty`, `--budget`, `--urgency`, `--rigor`, persisted on the task and accepted by `aid run`, `aid advise`, and batch TOML.
- New `aid advise "<prompt>"` prints the agent aid would choose, with a per-term score breakdown, cost and duration estimates, and the reason every other candidate fell short. It dispatches nothing and writes nothing.
- New `aid agent list --json` and `aid agent show <name> --json` expose the fleet inventory: install state, quota and reset time, per-category history, available models, and current load.
- New MCP tools `aid_agents` and `aid_advise`, and `aid hook session-start` reports agent quota state when any agent is limited.
- New built-in agent `grok`, wired from captured CLI behaviour: it reports its real model, tokens and cost, which most adapters do not.
- Dispatched agents can now delegate: a descendant task may re-enter an ancestor's worktree lease, `AID_TASK_ID` links sub-tasks into the task tree, nesting is capped at depth 2, and a child may not exceed its parent's declared budget or difficulty.
- Quota exhaustion is recognised per provider from captured wording — codex, droid, qwen, agy and oz each phrase it differently, and none matched the previous generic phrase list. Reset times parse four real formats instead of defaulting to "~1h".
- agy meters model families separately: an exhausted gemini allowance no longer takes its working claude allowance out of rotation, and dispatch switches to a healthy family.
- The idle watchdog no longer counts aid's own nudges as agent activity, so a stalled task is detected instead of running to its hard timeout.
- Auto-cascade is category-aware and skips unhealthy routes, including a gemini account that no longer serves individual tiers.
- Advice eligibility is a ranking penalty rather than a hard gate, so a caller always sees usable alternatives with the reason each fell short.
- `aid build` falls back to a writable target directory when the configured one is not, so agents under a sandbox can verify their own work.
- The model catalog is refreshed from live CLI output: current defaults such as codex's gpt-5.6-sol are present, oz gains a profile row, and a task whose model is unknown reports unknown cost instead of $0.00.
- qwen works again: it no longer receives flags its CLI does not accept, its model comes from the configured plan rather than a byte-order accident, and session resume is supported.
- The cursor adapter identifies its binary by asking it: another vendor installing a binary named `agent` was silently hijacking every cursor dispatch. Its default model is composer-2.5; composer-2 has been delisted.


## v9.12.2 (2026-08-05)
- Show which files an oz task touched: `tool_call` events carried only the tool name, so every `edit_files` call rendered as the bare string `edit_files` in `aid board`, `aid show --events` and the TUI while the event's own `file_paths` and `title` were discarded
- Build oz tool details from the title plus every touched path, attach the paths as `files` metadata, and classify `edit_files` as a file write
- Preserve file activity in task fallbacks: `aid export --sharegpt` fallback and a failed task's salvaged `partial-work.md` matched tool calls only, so a task that just edited or just read files reported that nothing had been recorded — both now cover file reads and file writes for every agent
- Split oz's inline tests into a sibling `oz_tests.rs` to stay under the 300-line file limit


## v9.12.1 (2026-08-04)
- Stop killing healthy Cursor tasks as "stuck in a loop"
- Fix Cursor event parsing picking the wrong key: `tool_call` objects now carry sibling metadata (`hookAdditionalContexts`, `startedAtMs`, `completedAtMs`, `toolCallId`), and taking the alphabetically first key collapsed every read/shell/write call into one identical event string, lost FileWrite/FileRead classification, and rendered every completed call as `completed: completedAtMs`
- Select the real tool entry by its `ToolCall` suffix, map write/edit/delete to FileWrite and read to FileRead, and keep unknown tools distinct by including the tool name and arguments in the key
- Remove repetition-counting loop detection entirely: it killed healthy tasks (8 identical events within 2 seconds was enough) while any non-pure pattern evaded it — a plain 8A/8B alternating loop survived 30 simulated minutes. Idle, hung-task, cost and maximum-duration safeguards are unchanged and remain the protection
- Correct the official guide on foreground idle and `--timeout`: idle detection waits on raw output lines, so unparseable output resets it despite no parsed activity, and `--timeout` is activity-aware rather than a hard wall-clock cap


## v9.12.0 (2026-08-04)
- Stop failing codex tasks that already delivered their report: the delivery guard counted codex's closing `todo_list` update as a work event, so a run whose report was written was still marked `missing_final_delivery` and its verdict discarded
- Stop erasing reports that contain a `[MILESTONE]` line: the log writer skipped any line containing that tag anywhere, and aid's own prompt asks agents to emit it, so a report opening with a milestone line never reached the task log
- Dispatch read-only audits as report tasks instead of implementation tasks: prompts phrased `read-only ... audit` are detected from the prompt alone, and such tasks no longer receive the implementer methodology or the git staging guard telling an auditor its deliverable is a commit
- Keep write-capable tasks fully scaffolded: scaffolding suppression requires explicit no-write intent, is clause-scoped around negated verbs (`do not modify` reads as an audit, `then fix the bug` does not), and is kept deliberately separate from dirty-worktree enforcement


## v9.11.0 (2026-07-31)
- Fix agy runs delivering tool narration instead of a report: aid no longer presents a result.md salvaged from the agent log as if it were the requested report, and records a missing-delivery assessment plus an `aid show` banner when the capture is pre-tool narration
- Fix agy `--add-dir` receiving relative paths, which agy rejects outright ("must be an absolute path") while keeping the unresolved entry in its workspace list; the command's working directory now resolves the same way, so sandbox and container mounts agree with the workspace paths
- Fix `aid run agy --model <name>` aborting immediately: agy has no `-m` short alias, so the long `--model` flag is now used
- Fix agy `--read-only` silently degrading to a prompt hint by probing for a plan-mode flag no agy version defines; `--mode plan` (agy 1.1.x) and `--approval-mode plan` (older) are now both detected, and flag probing requires the flag to open a help line rather than appear anywhere in it
- Tell agents that a `[MILESTONE]` line is progress, never a deliverable, and that a turn must not end on one


## v9.10.0 (2026-07-27)
- Ship the comprehensive, release-matched AID operating guide as the built-in `aid-guide` skill
- Refresh the official guide through `aid init`, `aid setup`, and `aid upgrade` while preserving user-owned skills
- Enforce guide maintenance with public-command coverage and installation lifecycle tests


## v9.9.0 (2026-07-27)
- Task artifacts now remain in custody after completion, failure, merge, doctor, clean, and stale-worktree recovery; implicit auto-GC and direct worktree deletion commands are removed.
- New `aid accept`, `aid reject`, and `aid gc --task` commands separate principal acceptance from execution status and make acceptance records append-only.
- Custody GC requires a clean, unchanged artifact manifest and recursively proves every superproject and submodule commit has both an object and a durable ref outside worktree-private Git storage.
- The #866 submodule-loss topology is covered by regression tests: a commit present only under a linked worktree's private `modules/` object store blocks deletion and preserves the worktree.
- Release hygiene no longer runs `git worktree prune` or recommends bulk branch deletion.


## v9.8.0 (2026-07-27)
- Codex tasks no longer report success merely because the CLI exits with code 0. AID now requires a substantive final message after the last tool or todo event, so investigations that consume their turn before writing the report are recorded as missing final deliveries instead of Done.
- Read-only Codex tasks with a missing final delivery automatically resume the captured Codex session once with a focused instruction to write the report from existing evidence. A repeated hollow turn fails cleanly without starting a third attempt.
- Completion summaries no longer promote an early progress update into the task conclusion when final delivery validation failed.
- End-to-end coverage exercises both successful same-session recovery and the repeated-hollow stop condition through a fake Codex CLI and the real task database.


## v9.7.0 (2026-07-26)
- A task that committed to its worktree branch can be continued again. Auto-GC prunes a finished task's worktree directory, after which both `aid run --worktree <branch>` and `aid retry` refused with "Branch <b> has N unmerged commit(s) not on main; refusing to force-reset (would orphan them)". The guard was right that a force-reset would orphan the commits and wrong that a missing directory implies a reset: the branch is intact and only needs its worktree recreated at the tip. This blocked the ordinary review-then-retry loop, and the remedy suggested in the error text read as if it would discard the work.
- `aid run --timeout` now states its unit. The flag had no value name and no help text while being interpreted as seconds, so `--timeout 120` intending minutes silently capped a task at two minutes and killed it with nothing produced.
- `aid show` no longer attributes the base commit's diff to a task that produced nothing. It fell back to `git diff --stat HEAD~1` when earlier ranges came up empty, so a task whose branch tip equalled its base displayed the base commit's own changes as if the task had made them — exactly the case where you most need the output to be trustworthy.
- End-to-end tests no longer inherit the developer's repository config. aid discovers project config from the working directory, so tests running at the repo root had every task they dispatched inherit ai-dispatch's own verify command; with verify set to `cargo test --bin aid` each dispatched task ran the full unit suite as its verification. Beyond removing the flakiness this makes the tests assert against a controlled environment, and it cuts the integration suite from roughly three minutes to about thirty-five seconds.
- This repository's own verify command is now `cargo test --bin aid` rather than `cargo check`, which did not build test targets at all.


## v9.6.0 (2026-07-26)
- Auto-retries no longer escape their worktree into the main repository. `retry_logic.rs` cleared the retry's worktree unconditionally but only set its directory when the worktree still existed, and auto-GC routinely prunes a failed task's worktree before its retry fires — so both were empty and the retry ran in the repo root. On 2026-07-25 that let an auto-retry commit an unreviewed reimplementation straight to `main`; twelve of that day's twenty retries lost a worktree their parent had, across several projects.
- The worktree isolation invariant is now stated once and enforced on the dispatch path instead of being re-derived in a conditional. `ensure_worktree_task_not_repo_root` refuses to launch a task carrying a `worktree_branch` into the repository root, and fails closed when the launch directory or repo anchor cannot be resolved rather than waving the task through. This defect had been patched eight times since June, each time by adjusting the same conditional.
- A retry whose worktree was pruned now recreates it at the branch tip, with `base_branch` set to the branch itself, so the parent attempt's commits survive and the existing force-reset refusal is not tripped.
- A failed verify now fails its task. Previously a task could record `verify_status = failed` while still reporting `status = done` with exit code 0, so anything trusting the status line or exit code — a reviewer, `aid board`, a script, a batch `depends_on` — proceeded on a lie.
- A configured verify that does not run is recorded as a failure with a reason instead of collapsing into `skipped`. A task ending through the dirty-rescue path used to report `skipped` and read as success. Genuine skips — no verify configured, no project file — still pass with exit code 0.
- The retry contract is now explicit: the attempt that failed verify keeps a `failed` record, while an `aid run --retry N` whose retry succeeds still exits 0, so `--retry` remains usable in scripts.
- The verify failure hint is gated on output that actually indicates missing dependencies. It previously suggested installing Node dependencies when a Rust project failed to compile.
- This repository's own verify command is now `cargo test --bin aid` rather than `cargo check`, which did not even build test targets. Every one of the eight prior fixes to the retry defect passed the old gate with a red suite.


## v9.5.0 (2026-07-25)
- New `aid build [check|test|clippy]` runs cargo for a dispatched agent and returns a compact digest instead of raw cargo output: a cold `cargo check --all-targets` with two errors drops from 140 lines / 4496 bytes to 3 lines / 185 bytes. Build progress goes to the task event stream (visible in `aid watch`, the TUI, and `aid board`) rather than into the agent's context, rate-limited to 3 messages with a configurable threshold and interval.
- `aid build` progress reports a compiled-unit count taken from cargo's `compiler-artifact` records, so a slow build is distinguishable from a wedged one. Repeated diagnostics are deduplicated but keep their multiplicity as an `(xN)` suffix; status-line error and warning counts remain unique-diagnostic counts.
- The Rust build cache is now shared across aid worktrees. A new branch's target directory is seeded from a shared `_base` with an APFS clone, bringing `cargo check --all-targets` from 37.13s cold to 8.45s; cloning 1.8 GB takes about 1 second and consumes 8 MB of real disk instead of 1833 MB.
- Clone availability is probed with `clonefile(2)` before seeding, so `cp -c` can no longer silently degrade to a full byte copy on non-APFS or cross-filesystem setups. Seeding records a `seeded` or `skipped` setup event with its reason and elapsed time.
- Branch target directories are seeded from pre-existing `debug/`, `release/`, and `.rustc_info.json` when `_base` does not exist yet, so the cache activates on machines that already have warm artifacts instead of waiting for a non-worktree build that may never happen.
- Branch target directories are now reclaimed when their worktree is removed or pruned and by `aid clean`, preserving `_base` and any branch with a live worktree. Previously nothing ever deleted them.
- Branch target directories stay namespaced inside the project's target root, so two projects using the same branch name no longer collide, and a branch can no longer be created beside unrelated projects' caches.
- `CARGO_TARGET_DIR` injection is centralized in one helper covering dispatch, background runs, verify, merge-verify, and container runs. Dispatched prompts for Rust projects state that a warm shared target directory is already configured and must not be overridden.
- `RUSTC_WRAPPER=sccache` was evaluated and rejected: the measured Rust cache hit rate across fresh target directories was 0.00% (1 hit / 114 misses, reproduced twice), because rustc invocations embed target-dir-specific absolute paths. The negative result is recorded in `docs/shared-cargo-cache-measurements.md`.


## v9.4.0 (2026-07-25)
- Fix: post-completion auto-retries of background tasks no longer fail instantly with `Failed during worktree setup: Not a git repository` — an explicit `--repo` is now honored instead of being discarded by an eagerly-evaluated `--dir` fallback
- Fix: a completed task's worktree is no longer pruned before aid decides whether to dispatch a verify/checklist/hang/model-selfheal retry, so the retry no longer inherits a directory aid just deleted
- Fix: retries whose worktree is gone now target the task's repository instead of a stale or empty directory, and refuse to dispatch when no usable directory can be determined
- Fix: `aid batch retry` keeps a task's configured subdirectory instead of silently redirecting the retry to the repository root
- Fix: retries preserve worktree path and branch metadata, so isolation is no longer lost across successive retry generations
- Safety: a task worktree can never be the repository's main checkout or the checkout it was dispatched from — `existing_worktree_path` skips the main working tree and `create_worktree` rejects any candidate equal to the repo path on every return path
- Safety: tasks whose stored worktree path equals their repository path are refused by retry, cascade, batch retry and merge, so historical records cannot commit into a main checkout


## v9.3.2 (2026-07-16)
- Fix: `aid run`'s process exit code now reflects the dispatched task's real outcome (0=Done, non-zero=Failed/Stopped/verify-failed) instead of always exiting 0 regardless of task result; `--bg` and `--dry-run` continue to exit 0 immediately as before
- Fix: foreground `aid run` completion now always prints one unambiguous, tagged status line (`[STATUS=DONE]`/`[STATUS=FAILED]`/`[STATUS=VERIFY_FAILED]`/`[STATUS=BG_RUNNING]`) so it can no longer be confused with a background-dispatch message, including when the task's own `--verify` step fails but the task status is kept as Done
- Fix: retry/cascade chains now propagate the correct final task ID back to the top-level `aid run` caller instead of the stale original task ID


## v9.3.1 (2026-07-16)
- Fix: `aid stop`/`aid kill` now release the task's `.aid-lock` on termination (including a race found in cross-audit where a still-Pending task's lock was skipped), so redispatching to the same worktree no longer permanently fails with "Worktree ... is locked" after a stopped or dead task (fixes #166)
- Fix: worktree reuse now uses a liveness-only lock preflight (`ensure_live_worktree_unlocked`) instead of a store-less check that could never recognize a stopped task's lock as stale, while still correctly rejecting reuse when a concurrent task's lock PID is genuinely alive
- Fix: `preserve_worktree`'s auto-commit failure is no longer swallowed silently — it now emits a warning
- Fix: `aid unstick` (default nudge mode) now checks worker/agent process liveness before sending a nudge and fails fast with a pointer to `--escalate`/`aid stop` instead of reporting false success against a dead worker (fixes #167)


## v9.3.0 (2026-07-09)
- Track each task's FINAL worktree HEAD/branch (agents that `git switch -c` to a new branch are now recorded correctly)
- `aid show` reports what was delivered: diff-stat + final commit subject + real final branch, with a prominent warning when the agent switched away from the dispatch branch
- `aid merge` now targets the agent's real final branch instead of the dispatch-time branch, preventing silent merges of the wrong (empty/stale) branch; drift requires `--force`
- Capture final state before worktree cleanup on all completion exits (done / fail / stop), so failed and stopped tasks also record their real branch
- schema: add nullable `final_head_sha` / `final_branch` columns (idempotent migration)


## v9.2.0 (2026-07-08)
- fix(reaper): terminalize awaiting_input/stalled tasks with dead workers — failure writes were SQL no-ops for those states, leaving tasks non-terminal forever; named status sets (ACTIVE_TASK_STATUSES / ACTIVE_EXECUTION_FAILURE_STATUSES) now shared by store and lifecycle
- fix(reaper): kill worker+agent processes unconditionally once a reap condition holds — kills were gated on the DB row transition, leaking processes when a concurrent path had already terminalized the task; aid stop now also kills Stalled tasks' processes
- fix(worktree): lease-based .aid-lock {task_id, owner_pid, worker_pid} — background workers re-key the lock to their own PID so it survives launcher exit; clears require matching task_id; lock checks are side-effect-free; stale-lock recovery re-validates captured content and restores a concurrently acquired fresh lock instead of clobbering it
- fix(lifecycle): route foreground max-duration timeouts and background worker errors through post_run_lifecycle — on_fail hooks, failed-worktree cleanup, retry/cascade, hung recovery and webhooks now fire on those paths; exactly-once completions.jsonl append
- refactor(store): split task_queries into metrics/similarity/worktree query modules (300-line rule compliance)


## v9.1.1 (2026-07-06)
- fix(tui): default `aid watch --tui` scope now shows active tasks from previous days as well as today's tasks, so a task that started yesterday but is still Running/AwaitingInput/Stalled/Waiting/Pending stays visible after midnight
- fix(output): stdout/transcript-only agents such as agy now autosave plain Markdown transcripts to task output and count transcript/log fallback content in hollow-output detection, avoiding false `hollow_output` assessments when the agent produced a substantive deliverable


## v9.1.0 (2026-07-04)
- feat(hang): first-token dead-stream detection — reap a streaming agent that emits no raw PTY output within 180s (env AID_FIRST_TOKEN_TIMEOUT_SECS) while still at zero real progress, instead of waiting the full idle timeout; gated to streaming agents and reset on any raw byte so live tasks are never falsely reaped
- feat(hang): transient auto-recovery — a first-token dead-stream hang auto-retries once with a fresh session (and first cascade agent, if configured) even without --retry, loop-capped via the retry-chain marker; ordinary idle hangs keep their existing gating
- fix(budget): parse daily/weekly/monthly (and day/week/month) budget windows — previously only Nh/Nd/Nm suffixes were understood, so a "daily" window silently fell back to counting all tasks ever (a lifetime cap masquerading as daily); also warn when a non-empty window is unrecognized
- fix(idle): skip the idle auto-nudge for non-interactive exec agents (codex) that never read stdin — such agents now go straight to escalate instead of emitting a useless nudge; new Agent::accepts_idle_nudge() capability
- fix(cost): price codex gpt-5.5 at the premium tier ($2.5/$15) instead of the generic gpt-5 rate
- fix(cost): correct the stale gpt-5.4 catalog price ($2/$12 standard -> $2.5/$15 premium) and stop mis-pricing gpt-5.4-mini as the flagship (it matched the "gpt-5.4" substring and was billed ~6x; now correctly $0.4/$1.6)


## v9.0.0 (2026-07-03)
- Architecture-audit release: 25 issues (#139-#151, #153-#164) from the 2026-07 three-lens architecture audit fixed and cross-audited; see docs/audit-architecture-2026-07.md for the full map
- feat(lifecycle): task status transitions are now guarded by a legal-transition graph with intent-named methods (task_lifecycle); failure salvage moved out of store mutations; background/batch tasks run the full post-run lifecycle (checklist retry, hooks, peer review, audits) via LifecycleMode
- feat(timeouts): one TimeoutPolicy resolved at dispatch replaces 14 scattered mechanisms; foreground max-duration is activity-aware (streams past the cap are no longer killed); idle detection counts any parsed event as liveness; hidden 300s fallback removed
- fix(pty): PTY pipeline no longer loses workgroup findings, re-saves session ids, or corrupts split multi-byte UTF-8; PTY logs strip escapes; both pipelines kill with TERM-grace-KILL; three PTY end-to-end tests added
- fix(retry): effective dispatch args persisted on the task and rehydrated by all retry paths — retries no longer silently drop team/context/scope/skills/worktree; cascade inherits the original worktree
- fix(budget): name-only budgets aggregate real usage scoped to their project and only gate that project's dispatches; token limits enforced
- fix(agents): agy read-only falls back to prompt-prefix instead of hard-failing; kilo/mimo rate limits no longer bench OpenCode; kilo/mimo declare needs_pty (piped stdout is swallowed, empirically verified); adapter layer collapsed via shared read-only helper, parse_completion default, and overlay delegate specs for kilo/mimo/qwen (net -170 LOC)
- feat(cli): real `aid wait` subcommand and watch --wait; --timeout/--exit-on-await honored; show --full/--events implemented with mutually-exclusive mode flags; event details keep full text in metadata past the 80-char cap; `completions` renamed to `notifications`; unknown [project] keys rejected
- refactor(layering): model catalog, worktree removal, hung recovery, task actions/views extracted from cmd/ — no lower layer imports CLI command modules; dispatch handlers grouped by verb domain; dead surface swept (~120 LOC) and ~/.aid/shared cleanup wired into aid clean
- fix(release): scripts/release.sh --dry-run is side-effect free; dispatch aborts cleanly when the status guard rejects a transition; malformed-lock cleanup TOCTOU closed


## v8.105.0 (2026-07-03)
- feat(agent): agents can now be disabled via `disabled = true` in agent_config.toml, managed by `aid agent config <name> --disable/--enable`; disabled agents are hidden from `aid config agents` (with a one-line summary), skipped by auto-selection, fallback chains, and team preferences, and explicit dispatch fails fast with an enable hint; `aid ask`/`aid explain` bail clearly when their default agent is disabled
- fix(watcher): strip OSC (BEL/ST-terminated) and CSI terminal escape sequences at the stream choke point before event parsing — droid >=0.159 prefixes window-title/progress escapes to its stream-json lines under a PTY, which made every droid task since 2026-06-29 fail or be killed as hung; root cause documented in docs/investigation-droid-osc-escapes.md
- feat(salvage): failed tasks with a worktree now deterministically write task_dir/partial-work.md (git status summary, diff stat incl. untracked, last activity before failure) and best-effort WIP-commit uncommitted changes on the task branch (hooks and signing disabled so the failure path never blocks)
- feat(show): `aid show` reports a live Worktree State section; untracked/staged-only partial work no longer renders as "(no changes detected)"
- fix(worktree): `aid worktree prune` skips worktrees with uncommitted changes instead of force-removing them


## v8.104.0 (2026-07-03)
- fix(audit): stdout-only agents (agy/gemini) and read-only audits no longer produce empty results — persist_result_file() now falls back to extracting the agent's final output from the event log into task_dir/result.md when the agent emitted its report to stdout instead of writing the file
- fix(export): aid export / .md export now reads the persisted task_dir/result.md when output_path is unset, so audit reports from stdout-only agents surface in exports too
- feat(retry): add --bg flag to aid retry for non-blocking dispatch, matching aid run --bg (previously retry always blocked in the foreground)
- feat(watchdog): raise the default idle timeout from 300s to 600s so agents are not falsely reaped during long silent phases like a cold cargo build (per-agent/per-task overrides still apply)


## v8.103.0 (2026-06-29)
- fix(tui): TUI now shows all task types — newly-dispatched Waiting/Pending tasks (and Skipped/Stopped) are no longer hidden in the multipane view; previously the status filter only kept Running/AwaitingInput/Stalled/Done/Merged/Failed, so a freshly-dispatched task stayed invisible until it started running
- feat(tui): add a Created timestamp column to the board table and a "Created {time}" line to each multipane pane (local time, %m-%d %H:%M)


## v8.102.0 (2026-06-29)
- feat(model): auto-heal when an agent fails because its selected model id is unavailable (deprecated/renamed/unsupported — e.g. opencode "Model not found", codex "model not supported", mimo 400). aid now detects this class of failure and automatically retries once forcing the agent's own current default model, so a stale model selection no longer hard-fails the task.
- feat(model): new `force_default_model` dispatch path bypasses smart-routing, budget, and configured defaults so the self-heal retry always lands on a guaranteed-valid model. Loop-guarded to run at most once per retry chain.
- fix(model): refresh stale built-in model tables — codex now uses gpt-5.5 / gpt-5.4 / gpt-5.4-mini (the old gpt-4.1 lineup was rejected by the API); opencode now uses provider-prefixed current ids (opencode/glm-5.2, opencode/kimi-k2.6, opencode/deepseek-v4-flash-free, opencode/nemotron-3-ultra-free, opencode/mimo-v2.5-free — the old bare glm-4.7 etc. were "model not found").
- fix(selection): budget mode now prefers free models — a paid model must be clearly stronger to win, so trivial budget-mode tasks route to free agents (kilo / qwen / free opencode) instead of a marginally better paid model.


## v8.101.1 (2026-06-29)
- fix(retry): resume agent sessions across the whole opencode family (opencode, kilo, mimocode) plus droid, not just opencode. Retry/iterate/post-done/verify/dirty-rescue now propagate the stored `agent_session_id` for every agent that can replay it via `--session`/`--continue`/`--fork` (or droid's `-s`), via a new `AgentKind::supports_session_resume()` capability.


## v8.101.0 (2026-06-29)
- feat(agent): add `mimocode` agent wrapping Xiaomi MiMo Code CLI (opencode-family fork). Dispatch via `aid run mimocode "<prompt>"`. Reuses the opencode JSON event parser like `kilo`; supports `--dir`, sessions, context files, and budget (`--variant minimal`).
- fix(mimocode): always pass an explicit `-m`, defaulting to `mimo/mimo-auto`. MiMo's own CLI default (`mimo-v2.5-pro-ultraspeed`) is server-rejected (HTTP 400), which had made every non-routed/complex dispatch fail immediately.
- fix(cost): scope MiMo zero-cost pricing to the native `mimo/` provider so paid models containing "mimo" (e.g. `opencode/mimo-v2-flash`) are no longer wrongly zero-costed.


## v8.100.13 (2026-06-29)
- fix(board): `aid board --json` is no longer suppressed by the anti-poll throttle. The cooldown/repeat/rate-limit guard ran before JSON was emitted and could exit early with a human-readable "[aid] Board checked Ns ago" hint or a non-zero status, so a script polling `--json` intermittently got empty output, non-JSON text, or exit(1). JSON mode now bypasses the throttle entirely and always prints a valid JSON array; the human (non-json) path keeps its unchanged anti-poll behavior.


## v8.100.12 (2026-06-29)
- fix(dispatch): idle watchdog now tracks real task progress (milestones/tool-calls) instead of raw PTY bytes — spinner/"thinking" chatter no longer masks a hung agent. Previously only Codex plain-text was safe; now correct across all PTY streaming agents (opencode/cursor/kilo/custom) whose status lines were classified as reasoning and reset the idle clock.
- fix(dispatch): zombie reaper backstop for a live worker whose monitor is wedged — a running task with no progress event beyond 2x its idle timeout is now reaped and its orphaned agent killed. AwaitingInput/Stalled tasks (legitimately waiting on a human) are never killed.
- fix(dispatch): terminal completion sentinel "=== AID TASK <id> DONE|FAILED ===" is emitted on every terminal path including wedged-worker kills, giving scripts a reliable done-signal instead of guessing from output.md growth or result.md presence.
- fix(show): aid show and board now print an explicit "Status: DONE/FAILED" for a terminal task that has no result file, so a finished task no longer reads as still-running.
- fix(run): interrupted foreground aid run/retry now converge — the task is recorded Failed (not left "running" for 24h) and the agent child is killed instead of being collateral-killed by PTY hangup with no status update. Foreground tasks now write a run spec so the orphan reaper catches them, and a completion can no longer overwrite an interrupt-set Failed status.


## v8.100.11 (2026-06-24)
- fix(dispatch): agents that commit all their work are no longer falsely warned "made no code changes in worktree". read_empty_diff only inspected uncommitted (git diff HEAD) and staged (git diff --cached) state, so a clean worktree with commits ahead of base evaluated as empty — the better-behaved the agent (commits everything, leaves a clean tree), the more reliably it was misreported. The check now also runs a three-dot base...HEAD committed-vs-fork diff, threading the task base branch through maybe_flag_empty_worktree_diff and the same-bugged maybe_flag_hollow_output, with a default-branch fallback (origin/HEAD then main then master) that degrades to the prior dirty-only behavior when no base resolves. Cross-audited SHIP; 4 new adversarial regression tests.
- fix(test): stop batch_refills_pending_tasks_when_slots_free_up from flaking under release-time load — the ~0.4s-ideal test asserted a 3s wall-clock bound that 8 binary spawns intermittently exceeded under parallel compile/test load (~66% pass rate); raised to a load-tolerant 15s ceiling that still catches a stalled refill.


## v8.100.10 (2026-06-24)
- fix(dispatch): orphaned background tasks now fail-fast on idle timeout instead of hanging "running" forever (a codex task sat idle 32 min until a manual stop). Background agents run under a detached PTY worker whose idle watchdog only polls while that worker is alive; if the worker exited or was killed while the agent child stayed orphaned, nothing converted "running but silent" into a hung failure. `check_zombie_tasks` now runs an independent orphan reaper that fails a task only when BOTH its supervising worker PID is dead/absent AND its latest event predates the task's dispatched idle timeout, then kills the orphaned agent child and records a hung-detected event.


## v8.100.9 (2026-06-07)
- fix(worktree): reusing a `--worktree` branch name no longer orphans the prior task's unmerged commit (#137). The fallback "existing branch" path in `create_worktree` ran `git branch -f <branch> HEAD` unconditionally when the worktree dir was pruned but the branch ref remained — silently orphaning unmerged commits (reachable only via reflog). The lifecycle made this routine: on completion aid prunes the worktree dir but keeps the branch, so same-name reuse always missed the safe `reconcile` path and hit the unguarded force-reset.
- New `reconcile::ensure_branch_force_reset_is_safe` resolves the base ref to a concrete OID once, refuses the force-reset with a clear `aid worktree remove` hint when `<base>..<branch>` has unmerged commits, and returns that OID so the same commit object is used for both the safety check and `git branch -f` — closing a TOCTOU window where symbolic `HEAD` could resolve differently between check and reset (found in cross-audit).


## v8.100.8 (2026-06-05)
- fix(sandbox): mount linked-worktree git directories in container `--sandbox` mode (#127). When `cwd` is a linked git worktree, `wrap_command` now bind-mounts the worktree's commondir (and the per-worktree gitdir only when it lives outside commondir) so `git add`/`git commit` inside the Apple-container sandbox can reach the shared object database — the symmetric fix to #126's codex-sandbox case.
- refactor: extract `resolve_worktree_gitdir`/`read_commondir` into a shared `src/worktree_layout.rs` used by both the codex adapter and the container sandbox; mounts are computed non-overlapping to avoid nested bind mounts.


## v8.100.7 (2026-06-05)
- fix(dispatch): task ID collisions (#134) — a 5-layer fix. `TaskId`/`WorkgroupId` widened from 16-bit (`t-{u16:04x}`, 65 536 values) to u32 hex (`t-`/`wg-{:08x}`), drastically cutting birthday-paradox collision odds.
- fix(dispatch): generated task IDs now insert-and-retry on a UNIQUE/PRIMARY KEY conflict (bounded loop, rusqlite extended code 1555) instead of erroring — previously regenerate-retry only ran for explicit `--task-id`.
- fix(dispatch): data-loss fix — the task row is now committed (claiming the ID) BEFORE any worktree/branch mutation, so a failed dispatch can no longer force-reset the target branch to base HEAD and orphan the prior commit.
- fix(dispatch): symmetric failure handling — every post-insert error path marks the task Failed atomically, and the worktree lock is released on all error paths via an RAII guard.
- fix(board): widen ID / Parent / Group columns to keep rows aligned with the new 8-hex IDs.


## v8.100.6 (2026-06-04)
- fix(dispatch): hollow-output guard now counts characters, not bytes (#131) — a 199-char/205-byte agent preamble was slipping past the 200-byte threshold, so zero-delivery audit tasks were silently marked Done with no HollowOutput flag. `output_content_length` now uses `chars().count()` in both branches.
- fix(dispatch): broaden audit-report detection to auditor-role prompts (#132) — an adversarial read-only audit prompt dispatched without `--read-only`/`--result-file` failed to engage the `## Findings` report flow. Added `strong_audit_intent` (auditor-role declaration or "audit ... against <baseline>") as a trigger; `skips_dirty_enforcement` stays strict and decoupled.
- test(gemini): serialize env-mutating trust-workspace tests (#133) — two tests mutating the process-global `GEMINI_CLI_TRUST_WORKSPACE` raced under parallel `cargo test`, intermittently failing `release.sh`. Now serialized via a shared mutex matching the existing `sandbox.rs`/`state_tests.rs` pattern.


## v8.100.5 (2026-06-02)
- fix(routing): guard `audit`/`review`/`verify` prompts from silent cheap-model downgrade. `is_simple_for_routing()` only denylisted `"security audit"`, so short audit/review/verify prompts slipped through as "simple" and got routed to the cheapest (nano) model — the wrong model for correctness-critical work. The denylist now uses substring `"audit"` (covers cross-audit/security-audit) plus `"review"` and `"verify"`; such prompts now defer to the agent's own configured model. No model version is pinned.


## v8.100.4 (2026-06-01)
- fix(dispatch): correct dirty-worktree baseline subtraction in `final_dirty_assertion` — it now subtracts the pre-task dirty baseline (matching `rescue`), so a user's pre-existing uncommitted files in a shared `--dir .` no longer flip a completed audit/report task from Done to Failed after its report was already written. Baseline-path parsing promoted to a shared `src/worktree/baseline.rs` helper used by both rescue and the final assertion.
- fix(dispatch): audit report-mode tasks skip dirty-worktree enforcement entirely (rescue + assertion), driven by a narrow `skips_dirty_enforcement` predicate. The skip is decoupled from result.md auto-set so a non-read-only write-intent task (e.g. "review and fix X" with --result-file) keeps its uncommitted-changes safety net.
- fix(dispatch): recompute and persist `log_path` when an explicit task ID collides and is auto-suffixed (t-ebcf -> t-ebcf-2); the worker previously wrote events to the original ID's log file.
- fix(dispatch): re-key the worktree `.aid-lock` owner to the suffixed task ID after an AutoSuffix collision, so lock ownership diagnostics are no longer stale under --worktree.


## v8.100.3 (2026-05-28)
- fix(agent/codex): include common gitdir in sandbox writable_roots — linked git worktrees share `.git/objects/` with the main repo, but the codex sandbox previously only had the per-worktree gitdir (`.git/worktrees/<name>/`) in `writable_roots`. Writes to the shared object database were blocked, codex's per-task gitdir-rewrite workaround didn't persist past sandbox teardown, and entire commits were silently lost on the worktree's branch — the SEV-1 class documented in #126 (290 LoC + 8 tests of iter-E2a wiped, no reflog trace, no fsck recovery). Fix: also include the common gitdir, resolved via the standard `<gitdir>/commondir` file. Falls back to current single-entry behavior if commondir is missing. Cross-audited by codex (t-fa15: SHIP) and gemini (t-4d12: noted container `--sandbox` mode needs symmetric fix in `src/sandbox.rs::wrap_command`, tracked as follow-up — that path triggers only when user explicitly passes `--sandbox` and is unrelated to the iter-E2a loss).


## v8.100.2 (2026-05-23)
- fix(log): clarify worktree-cleanup messages across 6 sites. The old `[aid] Removed completed worktree {path}` (and its peers) read like aid destroyed the agent's work, when in reality those cleanups only run after commits are safely on the branch (or only on empty branches that fast-failed in <10s). New wording leads with the safety guarantee (or honestly states what's discarded) and adds a restore hint where applicable. Affected logs: `cleanup_completed_worktree`, `maybe_cleanup_fast_fail_impl`, `remove_worktree` (git + fallback path), `aid worktree prune`, `aid clean --worktrees`, and `aid retry --reset`. Cross-audited by codex (t-9808).


## v8.100.1 (2026-05-20)
- fix(agent/antigravity): probe `agy --help` once per process (cached via `OnceLock`) so the adapter auto-unlocks `--approval-mode`, `-m`, and `stream-json` when upstream agy ships them — no aid redeploy required.
- fix(agent/antigravity): tiered `read_only` fallback — when agy lacks plan mode but `--sandbox` is on and the container CLI is available, aid lets the container enforce read-only at the OS layer instead of hard-bailing. Without sandbox, the error now names both `--sandbox` and `gemini` as concrete next steps.
- fix(agent/antigravity): `parse_completion` now attributes `gemini-3-pro-preview` and `$0.00` (Google One free tier) so `aid stats` and usage rollups are no longer blind to agy runs.
- feat(run): expose `RunOpts.sandbox` so adapters can branch on container availability (threaded through CLI, batch, background, and all `RunOpts` construction sites).


## v8.100.0 (2026-05-20)
- feat(agent): add Antigravity CLI (`agy`) as a first-class agent — Google is migrating Gemini CLI users on Google One / Gemini Code Assist (individuals) to the new `agy` binary; the legacy `gemini` CLI stops serving those tiers on June 18, 2026. The new adapter is non-streaming (plain text, no model flag, no plan mode) with a long `--print-timeout 24h` so aid keeps managing task timeouts. `agy` is wired through detection, binary preflight, adapter dispatch, setup wizard, usage rollups, `aid config agents`, selection scoring, auto-skills (researcher), TUI color, and the `AgentKind::Antigravity` enum. Paying API users keep using `gemini`; both adapters coexist.
- fix(agent/antigravity): bail with a clear error when `--read-only` is requested against `agy` (1.0 has no plan mode and would otherwise hang on an unanswerable permission prompt), warn when `--model` is supplied (silently ignored by `agy` 1.0), and make agy's own `--print-timeout` long enough that aid's task timeout always fires first.
- fix(tui): change Antigravity color from `LightGreen` to `LightYellow` — was previously colliding with Copilot in chart series.
- chore: gitignore `.antigravitycli/` session marker that `agy` writes into the workspace root.


## v8.99.11 (2026-05-20)
- fix(report-mode): narrow audit-keyword auto-detect to gated, word-boundary matching — prompts like "Add an audit log feature", "Redesign the audit subsystem", or "Implement the requested fix" no longer wrongly trigger audit-report mode, which used to silently inject `<aid-result-file>` plus a "produce a Markdown audit report starting with `## Findings`" instruction into the prompt. Detection now requires `read_only=true` OR an explicit `--result-file` AND a word-boundary match on AUDIT_TERMS (was: any `.contains()` substring on a lowercased prompt). AUDIT_NOUN_PHRASES (`audit log`, `audit trail`, `add an audit`, `add audit`) are stripped before audit-term matching, so legitimate audits that mention an audit-log feature ("cross-audit the audit log feature") still trigger correctly. New regression tests cover uppercase / leading whitespace / punctuation / hyphenated terms / strip-then-match overlap; the bug was discovered when an `aid batch` for an implementation task whose spec said "This is an implementation task, not an audit" was nevertheless mutated into a report-only task and produced zero code changes.
- fix(board): surface delivery_assessment in text `aid board` — terminal tasks with `delivery_assessment=empty_diff` or `hollow_output` now render a `[delivery:empty_diff]` / `[delivery:hollow_output]` suffix in the default text board, not only in `--json` and `aid tree`. A 0-diff task that burned thousands of tokens is no longer indistinguishable from a finished implementation. Includes a refactor of `src/board.rs` 431→296 lines via `src/board/detail.rs` extraction to satisfy the project's 300-line-per-file rule.
- fix(watcher): persist exit_code in foreground streaming success AND loop-kill paths — `watch_streaming` was writing the exit code to the completion event text but not to `tasks.exit_code`. The loop-detector kill path also bypassed the wait+exit-code finalization via early `return Ok(info)` (now `break`), so loop-killed foreground streaming tasks landed with `exit_code: None`. Both paths now correctly persist `info.exit_code` before returning, matching what the background PTY watcher (`src/pty_watch.rs`) already did. New tests cover the normal-success and loop-kill paths.
- docs: ship `docs/aid-improvements-2026-05-20.md` + companion `docs/research/2026-05-20-code-survey-codex.md` — comprehensive v9.0 planning analysis. Surfaces 5 new code-level findings (codex `-s workspace-write` flag never threaded through; cross-agent permission inconsistency where `read_only` means different things to Cursor/Gemini/Codex/OpenCode; audit-report classifier opacity — now partly fixed by the report-mode change above; `delivery_assessment` hidden from the default text board — now fixed; intent-vs-artifact mismatch unobserved), re-triages the 14 open items from `docs/ux-debt.md` against the v8.95–v8.99 CHANGELOG, names 4 recurring v8.95–v8.99 themes (worktree primitive instability, watcher loop-detector key gaps, BYOK permission plumbing, half-built audit-report scaffolding), and proposes a v9.0.0 must-fix list. Full file:line citations in the companion artifact.


## v8.99.10 (2026-05-13)
- fix(worktree): atomic lock acquisition closes a concurrent-dispatch race that let two `aid run --worktree <same-branch>` invocations against the same repo silently share the same physical worktree directory and clobber each other's commits. Real-world incident: parallel codex agents (P1/P2/P3) all wrote to `~/.aid/worktrees/<repo>-<hash>/<branch>` and rebased each other away, losing P2/P3 commits entirely. Root cause had two layers: (1) `create_worktree` returned an existing worktree as `created = false` even when another task was actively writing (designed for resume, not concurrent use); (2) the `.aid-lock` mechanism in `run_dispatch_prepare` was TOCTOU — `check_worktree_lock` then `write_worktree_lock` are two separate syscalls, so two racing dispatches both passed the check and both wrote, second overwriting the first. Fix in `src/worktree/state.rs` replaces the check-then-write with `try_acquire_worktree_lock`: write the full `task=…\npid=…` content to a unique temp path `.aid-lock.tmp.<pid>.<nanos>.<counter>`, then `std::fs::hard_link(temp, .aid-lock)` to atomically commit (fails `AlreadyExists`/`EEXIST` if target exists). Invariant: `.aid-lock` either does not exist or contains a fully-written task/pid pair — no partial state. On `AlreadyExists`: dead-pid locks recover via existing `check_worktree_lock` auto-clear; pre-fix malformed locks (empty/legacy crash residue) are atomically taken via `rename(.aid-lock, .aid-lock.malformed.<unique>)` so only the rename winner removes the file (loser sees `ENOENT` and retries hard_link). Three `create_worktree` reuse paths now call `ensure_worktree_unlocked` before returning an existing worktree, so concurrent callers get a clear `locked by task X — concurrent access prevented. Use separate worktree names for parallel tasks.` error instead of silent sharing. `clear_worktree_lock` now also sweeps orphan `.aid-lock.tmp.*` and `.aid-lock.malformed.*` files left by crashed acquisitions. Regression coverage in `src/worktree/lock_tests.rs`: empty-legacy-lock recovery, multi-threaded malformed-cleanup race asserts exactly one winner + correct holder report on the loser, orphan temp/malformed sweep, and `create_worktree` reuse rejection on a live lock. Three rounds of adversarial cross-audit (codex) closed an initial check-then-write TOCTOU, then a partial-write window between `create_new` and `write_all`, then a malformed-cleanup race where two processes both observed `holder.is_none()` and both called `remove_file`. POSIX `rename(2)` + Rust `std::fs::hard_link` semantics verified against man7/Apple/rustdoc.


## v8.99.9 (2026-05-07)
- fix(byok): align embedded MiMo manifest with vendor spec — `mimo-v2.5-pro` / `mimo-v2.5` now declare `context = 1048576` (1M) and `output = 131072` (128K), up from the stale `131072 / 8192` defaults that were truncating long-output agent tasks at 8192 tokens. The `output` field flows through `scripts/aid-byok-lib.sh` into `opencode.json`'s `limit.output` and is sent as `max_tokens` by `@ai-sdk/openai-compatible`, so the prior value was an active cap, not just metadata. Authoritative numbers cross-verified against OpenRouter model registry, HuggingFace MiMo-V2.5-Pro/V2.5 model cards, and Pi catalog. Manifest also gains two preamble notes documenting the >256K pricing-tier doubling and opencode's `OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX` (default 32000) — users wanting the full 131072 ceiling must raise that env var.


## v8.99.8 (2026-05-04)
- fix(droid): include tool args in event detail + populate `metadata.command` so `LoopDetector` keys per-target. Previously every droid `tool_call` event was logged with `detail = tool_name` only (e.g. `"Read"`) and no metadata, so 8 consecutive Reads of *different* files all hashed to the same key and false-positive tripped the loop kill — `t-3601` was killed at 6m17s mid-legit-exploration of a multi-crate fix. Adapter now matches `cursor.rs:138-187`: detail becomes `"Read /abs/path"`, `"Bash <cmd>"`, `"Grep <pattern>"`, and `metadata.command` carries the per-target signature consumed by `raw_event_key`. 2 regression tests added.


## v8.99.7 (2026-05-04)
- fix(droid): default to `--skip-permissions-unsafe` in non-read-only mode. `--auto high` still hit "insufficient permission to proceed" failures during headless aid runs and droid itself recommended escalation in the failure text. aid worktrees are sandboxed by branch and the caller has opted into autonomous orchestration, so the adapter now aligns with how aid already invokes other agents (`gemini -y`, `cursor --trust`). Read-only mode keeps using `--use-spec` and must not be silently upgraded.


## v8.99.6 (2026-05-03)
- fix(custom-agent): `CustomAgent::kind()` now returns `AgentKind::Custom` instead of always claiming `Codex`, so per-BYOK stats and rate-limit markers stop being misattributed to codex. The dead `AgentKind::Custom` branches in `background.rs:547` and `skills.rs:266` are finally live.
- fix(custom-agent): `build_command` mutates the prompt with the read-only / result-file prefix (mirroring opencode), so audit-report tasks dispatched to BYOK agents actually see the "DO NOT modify files except result.md" instruction. Previously the prefix was dropped on the floor and weak models silently dumped unstructured text on stdout.
- feat(byok): generated agent TOML now sets `streaming = true` + `output_format = "jsonl"` and the bash wrapper passes `--format json` to opencode, so the structured event stream reaches `aid show` and the TUI instead of arriving as a single trailing blob.
- feat(byok): `protocol = "openai"` manifests now emit `delegate_to = "opencode"` + `forced_model = "<id>/<model>"`. The new `OpenCodeOverlayAgent` (`src/agent/opencode_overlay.rs`) wraps `OpenCodeAgent` with the model pre-pinned, so mimo and other openai-protocol BYOK agents inherit opencode's read-only enforcement, result-file plumbing, and rate-limit handling for free instead of going through the bash wrapper.
- feat(show): `aid show <task-id>` and `aid show --result` surface a "Structured audit result missing" banner with a retry hint when an audit-style prompt finishes without producing `result.md`, instead of rendering the truncated raw agent log. New helper `report_mode::prompt_is_audit_report()` keeps the detection in one place.
- fix(gemini): the gemini adapter sets `GEMINI_CLI_TRUST_WORKSPACE=true` on the spawned command (unless the caller has already set it), so headless runs inside aid worktrees stop getting silently downgraded to "default" approval mode. Tests cover both the default-injection and override-respect paths.
- compat: existing BYOK installs need `scripts/aid-byok-apply.sh <manifest>` to regenerate `~/.aid/agents/<id>.toml` — the new `delegate_to` / `forced_model` / streaming flags only land via re-apply.


## v8.99.5 (2026-05-03)
- feat(gemini): auto-detect latest gemini model from task DB instead of hardcoding gemini-2.5-flash. New `Store::latest_default_model()` queries the most-recent successful task's model; pricing fallback now picks gemini-3-flash-preview by default and `aid config agents` shows a `Recent:` line listing observed models that are not in the static registry. Adds explicit gemini-3.x preview pricing entries (`gemini-3.1-pro-preview`, `gemini-3-flash-preview`, `gemini-3-flash-lite-preview`) and version-agnostic `flash`/`pro`/`flash-lite` aliases. The legacy `gemini-2.5-*` entries are preserved for historical task lookups.
- feat(config): `aid config agents` silently refreshes `~/.aid/pricing.json` from `https://aid.agent-tools.org/api/pricing` when the file is missing or older than 24h; gated by `cfg(not(test))` and overridable via `AID_NO_PRICING_REFRESH=1`.
- refactor(cost): split `src/cost.rs` into `src/cost/{mod.rs,pricing_builtin.rs}` to keep each file under the 300-line cap; pricing-table substring matchers now live in `pricing_builtin::for_model_lower`.


## v8.99.4 (2026-05-01)
- fix(watcher): include codex command in loop-detector key for ToolCall events — fixes false-positive loop kills when codex runs many distinct shell commands sharing an 80-char truncated prefix (e.g. `nl -ba <different-paths>`); same class as v8.99.3's FileWrite fix


## v8.99.3 (2026-05-01)
- fix(codex): stop false-positive loop kills when bursty file_writes are 80-char-truncated to the same prefix — LoopDetector now keys on raw paths with a 15-write threshold, while non-file_write events still trip on 8/10 identical untruncated keys (#125)
- fix(codex): tighten command-output classifier — `error[E<digits>]` line-prefix, `test result: FAILED`, and line-anchored `FAILED` only; substring matches in vendored crates / rg output no longer create fake Error events
- fix(tui): surface the real failure cause in the Reason column — pick the Error event whose detail matches a trigger phrase (`stuck in a loop`, `apply_patch`, `command failed`, `rate limit`, `killed:`, `task killed`, `exceeded ceiling`) before falling back to the first Error
- feat(watcher): on loop-kill, best-effort scan captured stderr for `apply_patch verification failed` and append it to the kill detail so the actual codex error reaches the board


## v8.99.2 (2026-04-30)
- feat(byok): add `aid byok` subcommand (`apply`, `remove`, `probe`, `example`, `doc`) — wraps the embedded BYOK shell scripts so cargo-installed users get the full opencode custom-provider flow without cloning the repo. The raw `scripts/aid-byok-*.sh` entry points remain as a lower-level fallback; env overrides (`OPENCODE_CONFIG_DIR` / `OPENCODE_AUTH_DIR` / `AID_HOME`) and exit codes are identical.


## Unreleased
- feat(byok): add bash+jq BYOK provider scaffolding for opencode custom providers, including apply/probe/remove scripts, a MiMo example manifest, sandboxed script coverage, and user docs for routing OpenAI-compatible providers through generated aid custom agents.


## v8.99.1 (2026-04-28)
- fix(commit): skip markdown bullets in rescue commit subject (#122, #123) — `extract_task_summary` now skips lines starting with `- `, `* `, `+ `, or `<digits>. ` in both the `[Task]`-section parser and the fallback loop. When neither pass yields a usable line, falls back to a generic `agent commit (task <task-id-short>)`. Previously, when a brief lacked an explicit `[Task]` header, the rescue commit subject would be the first injected `[Team Knowledge]` markdown bullet, truncated to 60 chars.


## v8.99.0 (2026-04-28)
- fix(watcher): kill process group + bound stderr drain in kill paths (#116, #117) — `watch_streaming` kill paths (idle timeout, cost ceiling, stuck-loop detection) now `force_kill_process_group` before draining stderr, and every stderr-capture handle await is wrapped in a 2s timeout via the new `drain_stderr_capture` helper. Previously, descendant processes kept the stderr pipe open after a kill, blocking `watch_streaming` from returning, leaving the task status stuck on `Running` and `aid watch --quiet` hung indefinitely. Extracted `force_kill_process_group` / `cleanup_process_group` into a shared `crate::process_group` module.
- fix(codex): include worktree git metadata in sandbox writable roots (#115, #119) — when codex is dispatched into a git worktree, `build_command` now resolves `<dir>/.git`, parses the `gitdir:` line, and appends `-c sandbox_workspace_write.writable_roots=[<canonical-metadata-path>]` so `git add` / `git commit` inside the codex sandbox can write to the parent repo's `.git/worktrees/<name>/index.lock`. Regular repos and missing `.git` no-op cleanly. Removes the rescue-commit churn that polluted git history with garbled messages.
- fix(worktree): protect active worktrees from prune + expose --json/--active (#114, #120) — `aid worktree prune` now reads `.aid-lock` and skips any worktree whose pid is alive, regardless of age. `aid worktree list --json` emits structured per-worktree records (`path`, `branch`, `active`, `lock_pid`, `lock_task_id`, `modified_age_secs`) for external tooling. `aid worktree list --active` filters human output to live-locked worktrees. README documents the `.aid-lock` contract for external cleanup tools.


## v8.98.0 (2026-04-28)
- feat(worktree): relocate aid-managed worktrees from `/tmp/aid-wt-{branch}` to `~/.aid/worktrees/{project-hash}/{branch}` so macOS `/tmp` cleanups no longer destroy in-progress work. Project ID is `{repo-basename}-{8-hex-hash-of-canonical-path}` to prevent same-basename repos from colliding. Old `/tmp/aid-wt-*` paths are still recognized by `aid worktree prune` and `aid clean --worktrees` for cleanup of pre-upgrade worktrees.
- fix(worktree): harden sandbox checks across `clean`, `merge_git`, `run_verify`, and `worktree_gc` — `is_aid_managed_worktree_path` now normalizes paths before prefix matching, rejecting traversal-shaped paths like `~/.aid/worktrees/../../etc`. Added a sandbox guard to `worktree_gc::remove_worktree_path` that previously ran `git worktree remove` on any DB-stored path without verification.
- fix(worktree): `aid run` invoked from inside a linked worktree now derives `{project}` from the main repo (via `git rev-parse --git-common-dir`) instead of the linked-worktree basename, so the resulting worktree lands under the correct project directory.
- chore(agents): hide `claude` from the default agent registry to keep `aid run auto` selection focused on agents with reliable headless execution.
- fix(test): update `retry_uses_fallback_when_rate_limited` to use Copilot instead of Claude in its pinned detected-agent set, since Claude was removed from the fallback chain in the same change above.


## v8.97.0 (2026-04-27)
- fix(tui): the FAIL "Reason" line now surfaces the FIRST Error event (the trigger), not the LAST. On cascade failures (loop kill → process failed → rescue → verify failed) users were seeing "Reason: Failed during verification ..." even though the real cause was the loop kill — making it look like verify failure was the trigger when it was just a downstream symptom.


## v8.96.0 (2026-04-27)
- fix(droid): stop emitting duplicate ToolCall events for `tool_result` and `tool_use` — these are already paired with `tool_call` and were doubling the LoopDetector input, causing false-positive loop kills (~5 legit reads → 10 events with detail "Read" → kill)
- fix(tui): render tool calls concisely in the Output tab — known primary keys (`file_path`, `path`, `directory_path`, `url`, `command`, `pattern`, `query`, `prompt`) are surfaced as `[Tool] <value> (k=v, ...)` instead of dumping the raw single-line JSON; unknown shapes still fall back to JSON, capped at 160 chars with an ellipsis


## v8.95.0 (2026-04-27)
- fix(droid): use `--append-system-prompt-file` for context files instead of `-f` (which means "read prompt from file" in droid and silently broke multi-context dispatches)
- fix(droid): `--read-only` now uses `--use-spec` (true read-only / spec mode) instead of `--auto low` (which still allowed file modifications)
- feat(droid): wire `RunOpts.session_id` to droid's `-s` flag for session continuity
- chore(droid): map `opus` shorthand to `claude-opus-4-7` (droid's own default), was stale at 4-6
- fix(worktree): re-anchor reused worktrees to the requested branch when an agent ran `git checkout` and steered HEAD elsewhere — was silently letting commits land on the wrong branch (#113)
- feat(stop): add `aid stop --retry-tree <id>` to cancel a whole retry tree in one call — resolves the argument to the chain root, walks every transitive descendant, stops every non-terminal member; composes with `--force` (#112)


## v8.94.0 (2026-04-20)
- feat(reply): new `aid reply <task-id> <message>` command — persists messages in a new `task_messages` SQLite table, PTY monitor delivers them to the agent's stdin and records ack when the agent produces output after delivery. `aid steer` now routes through the same persisted path.
- feat(unstick): new `aid unstick <task-id>` command — manual recovery for hung tasks. New `TaskStatus::Stalled` variant plus an `IdleDetector` policy module; the PTY monitor auto-nudges at warn threshold and escalates to `Stalled` past the escalation threshold.
- feat(batch): `aid batch` auto-prunes aid-owned worktrees when tasks complete successfully. Failed and shared worktrees are preserved. Opt-out via `.aid/project.toml`'s new `keep_worktrees_after_done = true`.
- feat(batch): on GitButler-active repos, `aid batch` completion and `aid watch --quiet --group` now print the `aid merge --lanes --group <wg-id>` merge-back hint alongside the existing `aid merge --group` suggestion.
- feat(batch): first `aid batch` invocation in a GitButler repo without `.aid/project.toml` integration prompts once to enable `gitbutler = "auto"`. Non-interactive / `--yes` / `--no-prompt` contexts skip the prompt; a `suppress_gitbutler_prompt = true` marker prevents re-prompting after a decline.
- feat(group): `aid group delete --cascade` deletes the group's member tasks transactionally rather than orphaning them. Without `--cascade`, the count of still-tagged historical tasks is printed with a pointer to `--cascade`.
- feat(merge): `aid merge --force` unblocks FAIL-status tasks that verify failed but have a clean working tree. Previously required hand-running `git merge`.
- fix(batch): `dir = "."` in a batch TOML now resolves relative to the TOML file's parent directory instead of the runtime's inherited cwd. First-wave tasks no longer fail with `Not a git repository: /tmp/.`.
- fix(background): missing agent binary now fails fast on the background dispatch path with the same clear preflight error the foreground path gives (GH#89). Shared `ensure_agent_binary_available` helper lives in `src/agent/mod.rs` and is called from both paths.
- fix(tests): workspace_dir test isolation — `/tmp/aid-wg-{id}` is now test-isolated via `AidHomeGuard` so parallel tests sharing workgroup IDs don't race on the same filesystem path. Production behavior unchanged.
- fix(tests): agent fallback tests now deterministic on CI hosts without agent binaries on PATH — new `DetectAgentsGuard` pins `detect_agents()` return value per-thread under `cfg(test)`.
- fix(clippy): clear 28 pre-existing `cargo clippy -- -D warnings` lints (rust-1.93 and rust-1.95 strictness). CI's build job is now green for the first time in several releases.
- docs: add `docs/gitbutler.md` covering integration modes, the batch → review → `aid merge --lanes` pipeline, the `AID_GITBUTLER=0` escape hatch, troubleshooting, and the `keep_worktrees_after_done` knob.


## Unreleased
- fix(gitbutler): completed worktree tasks now auto-prune their aid-owned worktrees by default when the branch has committed changes, while preserving failed tasks, shared worktrees, and projects with `keep_worktrees_after_done = true`
- fix(batch): `aid batch` now offers a one-time GitButler enable prompt for detected GitButler repos without `.aid/project.toml` integration, with `suppress_gitbutler_prompt = true` and `--yes` / `--no-prompt` escape hatches for non-interactive runs
- fix(gitbutler): batch completion and `aid watch --quiet --group` now surface the GitButler lane merge-back path via `aid merge --lanes --group <wg-id>`
- docs: add `docs/gitbutler.md` covering modes, batch review flow, `AID_GITBUTLER=0`, troubleshooting, and `keep_worktrees_after_done`

## v8.93.0 (2026-04-18)
- feat(release): `scripts/release.sh` now pre-flights orphan branch and orphan worktree detection. Branches merged into `main` and worktrees pointing at merged or missing branches block the release unless `--skip-hygiene` is passed. Dry-run mode only warns.
- feat(hygiene): new `scripts/session-preflight.sh` surveys repo health at session start — fetch, ahead/behind vs `origin/main`, dirty count, aid zombie tasks, aid worktrees for current repo, /tmp disk usage. Wired as a Claude Code SessionStart hook when `.claude/settings.json` enables it locally.
- docs: `docs/ux-debt.md` records 14 UX debt items grouped by severity plus five non-negotiable principles (resource lifecycle, path-relative-to-file, cross-repo safety, error translation at config layer, board truthfulness) for the v9.0 overhaul.
- docs: `docs/roadmap.md` and `docs/design/reply-unstick-spec.md` track the unreleased port work (reply/unstick/GH#89 background preflight) and the v9.0 plan. The reply/unstick feature spec is preserved for the follow-up port — see `ai-board` item `wi-273e`.


## v8.92.0 (2026-04-17)
- fix(verify): detect when a task prompt declares new files (`Create a NEW file: <path>`) but the resulting commit does not add them — verify now fails with the missing paths instead of silently passing (closes #103)
- feat(doctor): new `aid doctor` sub-command lists prunable worktrees and deletable merged/rebased branches under aid-managed prefixes; `--apply` runs `git worktree prune` + `git branch -d` (never `-D`)
- feat(auto-gc): opt-in auto cleanup of fully-merged task worktrees + branches on task/group completion via `--auto-gc` flag or `aid_gc = "auto"` in `.aid/project.toml` (closes #104)


## v8.91.1 (2026-04-17)
- fix(rescue): never amend tagged release commits — creates a new commit on top instead when HEAD has any tag (closes #102)
- fix(rescue): honor pre-task dirty-file baseline so the user's pre-existing uncommitted work is never staged/committed by rescue
- fix(rescue): exclude aid-internal artifacts (`.aid/`, `result-t-*.md`) from rescue staging
- fix(rescue): baseline handles rename/delete/kind-transition status lines correctly (path-only match)


## v8.91.0 (2026-04-16)
- refactor: split delivery assessment from verify status and persist it as first-class task metadata, including migration of legacy hollow-output and empty-diff states
- refactor: add a shared worktree snapshot boundary and reuse it across dirty checks, post-run settlement, commit, and rescue flows
- refactor: extract lifecycle phase decisions for teardown, escape checks, worktree settlement, verify/scope handling, checklist handling, and task post-processing
- fix: isolate agent rate-limit marker tests and ignore local `.aic/` audit artifacts so release status checks stay clean
- chore: unblock release gates by sharing Gemini-family support code through one module path and making the current clippy warning policy explicit


## v8.90.0 (2026-04-16)
- fix: `aid board` anti-poll enforcement strengthened — blocked states no longer output board data, repeat limit lowered to 1, hard blocks exit with code 1, hints include running task IDs


## v8.89.0 (2026-04-14)
- fix(#102): `should_rescue_path` no longer excludes `result-*.md` files — audit/cross-audit tasks that write result files are now properly rescued instead of triggering a guaranteed dirty-worktree FAIL
- fix(#102): `persist_result_file` now runs before Failed-task worktree cleanup, so result files are saved to `~/.aid/tasks/<id>/` while the worktree still exists


## v8.88.0 (2026-04-14)
- fix(#99): `prompt_scan.rs` no longer panics on multi-byte UTF-8 characters (em-dashes, arrows, etc.) in context files during `--dry-run`. Replaced byte-based `truncate()` with char-based truncation in `truncate_snippet`.
- fix(#97): batch cost total no longer double-counts — was exactly 2x the real sum because `waiting_ids` and dispatched `task_ids` overlapped. Now deduplicates before summing.
- fix(#96): `read_only = true` + `worktree` combination in batch TOML is now caught at parse/dry-run time with a clear error, instead of silently failing at dispatch after 30+ minutes.
- fix(#100): batch `--parallel` no longer serializes same-agent tasks. The auto-concurrency cap was limited to unique agent count (1 for all-codex batches); now uses CPU-based `recommended_max_concurrent` (4-24) capped at task count.
- fix(#101): `aid group finding add` no longer fails when called by codex agents in background tasks. Stopped auto-reading stdin (which is `/dev/null` in background) when content arg is missing; now requires explicit `--stdin` flag.


## v8.87.0 (2026-04-12)
- fix(#95): stop silent data loss when agents forget to `git add` new files. aid already ran `rescue_untracked_files` post-agent, but the defense had four gaps: it only handled `??` untracked files (modified-but-unstaged tracked files fell through), it amended the last commit and silently failed when the agent made zero commits, `git status --porcelain` collapsed fully-untracked directories to `src/` hiding individual files, and there was no final assertion before marking the task DONE. Now `rescue_dirty_worktree` (new, in `src/commit/rescue.rs`) covers both untracked and modified tracked files, uses `--untracked-files=all`, creates a fresh commit when HEAD is empty, and emits loud milestone events. A shared `post_agent_dirty_worktree_cleanup` helper runs rescue → retry → final assertion on BOTH the foreground (`aid run`) and background (`aid run --bg`) paths; if the worktree is still dirty after rescue and retry, the task transitions to Failed with a listing of remaining paths instead of silently losing them on worktree cleanup. Read-only audit tasks bypass the assertion by design. The injected `[Git Staging Rule]` prompt wording is now explicit: agents are told to run `git status --porcelain` before every commit and that any task leaving unstaged files will FAIL. Closes #95.
- feat(#98): opt-in `--audit` flag on `aid run` that dispatches `aic audit <task-id>` as a foreground subprocess when a task reaches DONE. Captures verdict (`pass` / `fail` / `error` / `skipped`) and report path as task metadata (`audit_verdict`, `audit_report_path`) and surfaces `Audit: <verdict> (report: <path>)` in `aid show` output when populated. Graceful degradation when `aic` is not on PATH — warning logged, verdict set to `skipped`, task status unaffected (audit is strictly informational; parent task status never changes based on auditor verdict). Configurable via `[audit] auto = true` in `.aid/project.toml` for per-project auto-audit, with a `--no-audit` CLI escape hatch to opt individual tasks out. Batch TOML supports `audit` at `[defaults]` and per-`[[task]]` levels with task-level overrides. Timeout default 5 minutes, configurable via `AID_AUDIT_TIMEOUT_SECS` up to 30 minutes. Closes #98.
- chore: split oversized touched files into submodules while fixing #98 — `src/types.rs` 795 → 67 lines (Task struct moved to `src/types/task.rs`), `src/project.rs` 581 → 296 lines (audit/team config extracted), `src/batch.rs` 575 → 170 lines (TOML schema and validate helpers extracted). Shared test env lock `crate::aic::test_env_lock` eliminates a race between `src/aic.rs` tests and `src/cmd/run_audit_tests.rs` tests that was producing flaky failures under parallel execution.


## v8.86.0 (2026-04-12)
- feat(qwen): add Qwen Code CLI (`qwen`) as a first-class aid agent. Qwen Code 0.14.x is a Gemini-CLI fork with identical stream-json output schema, so the adapter mirrors the Gemini one (stream events, tool call classification, token accounting). Default model is `coder-model`; free-tier pricing via OAuth or Alibaba Cloud Coding Plan. `aid run qwen "..."`, `aid batch` with `agent = "qwen"`, stats, board, and smart routing all work. Wired through `AgentKind`, adapter registry, selection scoring, cost table, rate limit tracking, container/sandbox matrix, and config models.
- fix(#94): strengthen worktree validation and stop running `but setup` inside task worktrees. `is_valid_git_worktree` previously accepted any git repo at the expected path — a standalone repo squatting `/tmp/aid-wt-*` would be silently reused forever, breaking merge-back. It now also requires the candidate's `git rev-parse --git-common-dir` to match the main repo's common dir AND the canonicalized path to appear in `git worktree list --porcelain` (with `/tmp` ↔ `/private/tmp` symlink aliasing handled). Separately, `run_dispatch_prepare` no longer calls `but setup` inside per-task worktrees — `but setup` requires the main worktree and the call was the most plausible trigger for the initial mutation. GitButler hooks now only wire for tasks when the main repo already has an active GitButler project; otherwise aid emits a one-shot hint telling you to run `but setup` from the main repo. Closes #94.
- chore: gitignore `.aid-verify-deps-state` and `result-t-*.md` so transient verify state and audit result files don't leak into commits.


## v8.85.0 (2026-04-11)
- fix(#91): detect nested git repos at dispatch time and warn loudly when the inner-vs-outer repo choice is ambiguous. New `--repo-root <path>` flag on `aid run` and `aid batch` (also `[defaults] repo_root = "..."` in batch TOML) to override auto-detection. Non-submodule nesting triggers a warning that names both repos, their remotes, and the exact override commands.
- fix(#92): `aid batch` / `aid run --worktree` now reconciles reused worktrees with the current branch HEAD before dispatch. When the reused worktree is behind and has no local edits, it is fast-forwarded automatically; otherwise dispatch fails with an actionable error (`aid worktree remove <branch>` hint). Verify-failure errors that were actually caused by a missing task directory inside a stale worktree now surface the real cause instead of a generic "verify failed".
- fix(#93): fresh worktrees no longer fail verify because `node_modules` / `target` / `.venv` are missing. New `setup` hook field in `.aid/project.toml`, batch `[defaults]`, and `[[task]]` — runs once per worktree (cached via `.aid-setup-done` marker) and streams output as `setup` events. When `setup` is not defined, aid falls back to symlinking well-known dependency dirs (`node_modules`, `target`, `.venv`, `venv`, `vendor`) from the main repo into the worktree, gated by a matching project file. Disable with `--no-link-deps` on `aid run` or `[defaults] worktree_link_deps = false`. Verify failures in fresh worktrees now append a hint pointing at the `setup` field.


## v8.84.0 (2026-04-10)
- fix(batch-retry): `aid batch retry <wg>` now serializes retried tasks that share a worktree. Tasks are bucketed by `(worktree_path, worktree_branch)`; buckets with more than one task dispatch sequentially and wait for each task to reach a terminal status before starting the next. Distinct worktrees still retry in parallel. Previously, shared-worktree tasks all dispatched concurrently and trampled each other.
- fix(commit): post-task `auto_commit` no longer scoops `.aid-lock`, `result-*.md`, or `aid-batch-*.toml` into stray commits. `git add -u` uses pathspec exclusion, untracked-file detection filters `result-*.md`, and the commit is skipped entirely via `git diff --cached --quiet` when nothing substantive is staged. Eliminates the "sandwich auto-commit" noise that every feature branch used to accumulate.


## v8.83.0 (2026-04-10)
- feat(gitbutler): opt-in GitButler integration. New `[project] gitbutler = "off" | "auto" | "always"` field, auto-detected by `aid project init` when the `but` CLI is present.
- feat(gitbutler): per-dispatch worktree integration — `but setup` runs in the worktree, Claude Code agents get `.claude/settings.local.json` with `but claude pre-tool|post-tool|stop` hooks, and non-Claude agents get an on-done `but -C <wt> commit -i` chained into `args.on_done`. Gated on `AID_GITBUTLER=0` escape hatch.
- feat(gitbutler): `aid merge --group <wg-id> --lanes` applies each task branch as a GitButler virtual branch lane instead of sequentially `git merge`-ing them, so a whole batch becomes a single reviewable workspace via `but status` / `but apply` / `but unapply`. Worktrees are preserved in `--lanes` mode.
- fix(background): `build_on_done_command` now routes commands containing shell metacharacters (`&&`, `||`, `|`, `;`, `>`, `<`, backticks, `$(`) through `sh -c` instead of naive `split_whitespace` + `Command::new`. Makes chained on_done commands actually work for any aid user, not just GitButler.
- fix(merge): `--lanes --check` and `--lanes --target` now return clear errors instead of silently ignoring the flag; `--lanes` without `--group` still errors cleanly. All three combinations have unit tests.
- fix(merge): `aid merge --group --lanes` now honors `AID_GITBUTLER=0` and the project `gitbutler` mode — previously the env var only gated dispatch hooks, letting `--lanes` still run.
- docs: new "GitButler Integration (optional)" section in CLAUDE.md covering modes, per-task behavior, escape hatch, and `--lanes` post-batch assembly.


## v8.82.0 (2026-04-09)
- fix: resolve relative `dir` and `context` paths in batch TOML against the batch file's location, not CWD


## v8.81.0 (2026-04-09)
- feat: Insights dashboard — `aid stats --insights` shows activity by day/hour, top sessions, overview totals with ASCII bar charts (H7)
- feat: Credential pool — `~/.aid/credentials.toml` for multi-key rotation per provider (round_robin/fill_first/least_used), `aid credential list` CLI (H2)
- fix: Rate-limit false positives — removed 503/payment from rate-limit classification, reduced TTL to 5min, auto-clear on success (#90)


## v8.80.0 (2026-04-09)
- feat: `aid export --sharegpt <task-id>` — export task conversations in ShareGPT JSONL format for fine-tuning (H4)
- fix: Rate-limit false positives — removed 503/payment from rate-limit classification, reduced TTL from 1h to 5min, auto-clear on task success (#90)


## v8.79.2 (2026-04-09)
- fix: `best_of` in batch no longer conflicts with running sibling copies — each copy gets unique task ID (#79)
- fix: Result file isolation — shared-dir batch tasks write to `result-{task_id}.md` instead of overwriting each other's `result.md` (#85)
- feat: `max_wait_mins` in batch TOML — WAIT tasks auto-fail after specified timeout, prevents indefinite hangs (#78)


## v8.79.1 (2026-04-09)
- fix: Smart routing 503 loop — detect "no plan" 503 errors as rate-limit, skip smart routing for rate-limited agents (#88)
- fix: `aid batch --quiet` hang — reconcile zombie tasks before polling completion, ensures exit when all tasks are terminal (#86)
- fix: Droid model shorthand mapping — map `haiku`/`sonnet`/`opus` to full model IDs required by factory-cli (#87)
- fix: Agent binary pre-flight check — fail fast with clear message when agent binary not found, instead of leaving task stuck in RUN (#89)


## v8.79.0 (2026-04-09)
- feat: Prompt injection scanning — context files and skills scanned for adversarial patterns (role hijacking, system prompt injection, invisible Unicode, XML tag injection) with warnings
- feat: Smart model routing — automatically uses cheaper models for simple prompts without --budget flag, configurable via `selection.smart_routing` (default: enabled), conservative heuristic (length, word count, code blocks, keywords)
- feat: Shared `embed_context_in_prompt` helper — context files now embedded in prompts for codex, cursor, oz, and codebuff agents (previously silently dropped)
- fix: Cursor read-only mode now passes `--trust` flag — fixes workspace trust prompt blocking plan-mode tasks
- fix: Oz read-only mode — added prompt-level enforcement (was completely missing)
- fix: Rate limit detection added for cursor, claude, opencode, kilo, and oz agents — enables cascade/fallback on quota errors


## v8.78.0 (2026-04-08)
- Fix `aid board` always showing data even when anti-poll triggers — warnings go to stderr, exit code 0 (#81)
- Fix `best-of-N` output file collision — each candidate gets isolated output paths, winner's files promoted (#82)
- Fix `aid batch --quiet` zero progress visibility — new `aid_progress!` macro emits flushed lifecycle events (#83)
- Fix batch concurrency limiter: better I/O-bound defaults (cpu_count clamped 4-24), `max_concurrent` in TOML defaults, agent diversity includes fallback targets (#84)


## v8.77.0 (2026-04-08)
- Strengthen anti-polling: remove `--force` bypass hints from board messages, add 30s force cooldown, escalating resistance (hard block after 3+ force calls in 2min)


## v8.76.0 (2026-04-08)
- Add time-based anti-polling cooldown (10s) for `aid board` — blocks rapid repeated calls with actionable hints
- Add `--force` flag to `aid board` to bypass anti-polling cooldown
- Tighten fingerprint-based repeat detection threshold from 3 to 2 identical checks
- Surface ETA estimation in regular `aid board` output for running tasks (was only in `--stream` mode)
- Add progress percentage display for running tasks based on historical median duration (capped at 99%)


## v8.75.1 (2026-04-08)
- Fix batch `best_of` dispatches reusing active task IDs and harden best-result selection
- Clarify the batch TOML rename from `timeout` to `max_duration_mins` in parser errors and docs
- Stop tracking local `.aid/state.toml` so personal state no longer blocks status checks or releases


## v8.75.0 (2026-04-08)
- Add GitHub Copilot CLI as a built-in agent with setup, selection, pricing, sandbox, and usage integration
- Improve Copilot log formatting in `aid show` and summary extraction across streaming and tool boundaries
- Refresh project documentation for supported agents and scripted release flow


## v8.74.1 (2026-04-08)
- Improve streamed CLI output formatting across `aid show`, TUI, and web views
- Fix Gemini response extraction for content arrays, tool boundaries, and revision-style text events


## v8.74.0 (2026-04-08)
- Allow read-only agents to write configured `result_file` outputs
- Fix read-only mode blocking persisted result files

## v8.73.0 (2026-04-08)
- Fix batch waiting-task cleanup for active workgroups
- Persist waiting-task retry configuration correctly
- Add JSONL event streaming for `aid watch` and retry support for waiting batch tasks

## v8.72.0 (2026-04-07)
- Cherry-pick mempalace memory upgrades: tiered memory injection and compact prompt format
- Add knowledge graph CLI and store support

## v8.71.0 (2026-04-07)
- Make `watch --group` track newly added pending and waiting tasks
- Keep active workgroup tasks visible in wait and watch flows

## v8.70.0 (2026-04-06)
- Retry agents on dirty worktrees instead of auto-committing
- Clear stale worktree locks during prune
- Auto-scope conflicting `result_file` paths in batch dispatch

## v8.69.0 (2026-04-04)
- Add Claude Code as a first-class agent with auto-selection support
- Update Cursor, Gemini, OpenCode, Kilo, and Droid adapters for newer CLI versions
- Improve agent selection scoring

## v8.68.0 (2026-04-04)
- Add `aid run --iterate N --eval CMD` generator-evaluator loop
- Integrate iterate mode with batch and background flows
- Add hung-task auto-recovery and split oversized run command modules

## v8.67.0 (2026-04-04)
- Add `--prompt-file` support for long prompts in run and batch tasks
- Support batch metadata fields
- Improve failure diagnostics and stale diff/worktree recovery

## v8.66.3 (2026-04-02)
- Fix OpenCode workspace access for workgroup directories
- Fix OpenCode output parsing and rendering in `aid show` and TUI

## v8.66.2 (2026-04-01)
- Add default audit report mode: review and cross-audit tasks now auto-write `result.md`
- Prefer persisted `result.md` in `show`, TUI, and web output views
- Fix TUI/web Codex output rendering to extract final assistant messages instead of raw JSONL logs

## v8.66.1 (2026-04-01)
- Fix Codex CLI v0.118.0 non-PTY runs hanging when stdin stays open
- Preserve `stopped` task status so timeout/completion writes do not overwrite manual stop

## v8.63.0 (2026-03-26)
- Detect output file conflicts in batch analyze (bail on guaranteed data loss)
- Auto-suffix conflicting output paths in parallel batch dispatch
- Expand file path detection to 16 extensions (md, json, toml, yaml, etc.)

## v8.62.0 (2026-03-26)
- v8.62.0: Support gemini-cli 0.35+ stream-json format
- Support gemini-cli 0.35+ stream-json format

## v8.61.0 (2026-03-26)
- v8.61.0: Fix changelog for crates.io installs + prominent upgrade banner
- Fix embedded changelog for crates.io installs

## v8.60.0 (2026-03-26)
- v8.60.0: Batch TOML parity with aid run flags
- Add missing `aid run` flags to batch TOML support. Currently
- Add missing batch TOML run flag support
- Custom ID conflict handling: block running, auto-suffix terminal

## v8.59.0 (2026-03-26)
- v8.59.0: Allow human-readable custom task IDs
- chore: auto-commit changes to .aid-lock
- Allow custom task IDs in dispatch flows

## v8.58.0 (2026-03-26)
- v8.58.0: Improve batch init template and changelog embedding reliability

## v8.57.0 (2026-03-26)
- v8.57.0: Fix TUI/web output display for custom agents
- Fix TUI/web "No output available" for custom agents with plain-text logs

## v8.56.0 (2026-03-26)
- v8.56.0: Show error reasons for failed tasks on board

## v8.55.0 (2026-03-26)
- v8.55.0: Code Health Round 4 — split 4 oversized files
- Split run_prompt into run_process and run_prompt_helpers modules
- Split src/tui/ui.rs (453 lines) into focused modules. Target
- Split show command into helpers, JSON, and test modules
- Split TUI ui into ui_detail and ui_tree modules
- Split agent module into env helpers and tests submodules

## v8.54.0 (2026-03-26)
- v8.54.0: Checklist Wave 2 — output scanning, auto-retry, show display
- feat: checklist Wave 2 — output scanning, auto-retry, show display
- Implement checklist output scanning in src/cmd/checklist_sca

## v8.53.0 (2026-03-26)
- v8.53.0: Sprint contracts — --checklist prompt injection (Wave 1)
- feat(run): add checklist prompt injection
- v8.52.0: Full output by default, read_only background fix, --json output field
- Preserve background read-only runs and AID_HOME
- Make show and output default to full content
- v8.51.0: Untracked file rescue, git staging guard, batch [[task]] alias, board anti-polling
- feat: rescue untracked files before verify, reorder background lifecycle
- fix: accept [[task]] alias in batch TOML, exit on repeated board polling
- Add git staging guard to writable prompts
- Add untracked file rescue helpers
- v8.50.0: Finding API, pending reason, read_only fix, idle timeout
- chore: remove stale aid-lock
- chore: auto-commit changes to .aid-lock
- chore: auto-commit changes to .aid-lock
- chore: auto-commit changes to .aid-lock
- Implement GitHub issue #68: expose pending-timeout reason in
- Add finding get/update commands and review fields
- fix codex read-only findings writes
- feat: increase default idle timeout to 300s and add per-agent config
- v8.49.0: Worktree safety and CLAUDE.md overhaul
- docs: update CLAUDE.md with full CLI coverage
- fix: prevent worktree contention from concurrent agent access
- v8.48.0: Reliability, dispatch intelligence, and UX polish
- Remove unused PTY idle-timeout constant
- Add configurable idle timeouts for runs and batches
- Skip rate-limited agents before batch dispatch
- Track new workgroup tasks during wait
- Fix GH#58: `aid board` anti-polling is too aggressive — reje
- Update Cargo.lock for v8.47.0
- v8.47.0: Codex CLI v0.116+ compatibility and TUI polish
- v8.46.0: UX fixes from dogfooding
- Add --limit flag to `aid board` to control how many tasks ar
- Reject unknown top-level batch keys
- Suppress dir warning for non-writing runs
- Reject unknown batch TOML fields
- v8.45.0: Project runtime state file (.aid/state.toml)
- chore: auto-commit changes to src/store/queries/state_queries.rs
- Refresh project state after task completion
- Inject recent project state into run prompts
- Add project state CLI command
- Create src/store/queries/state_queries.rs (~100 lines) with
- Add project state management module
- docs: add Show section to CLAUDE.md for research task output
- v8.44.0: Research task output improvements for aid show
- Relax research output truncation
- Auto-save research task output after completion
- Show research findings for no-change tasks
- v8.43.0: Fix read_only batch false positive merge conflict warning (GH#60)
- v8.42.0: Context pollution reduction — summary tools + smart skill injection
- feat: skip skill methodology/gotchas for short prompts (<200 chars)
- feat: summary-only tool injection — name + description, no command/args
- v8.41.0: Smart tool injection + per-category agent routing
- Track task categories for category-aware agent history
- Filter toolbox injection by task category
- v8.40.0: Fix cascade fallback for quota exhaustion (GH#57)
- fix: cascade fallback for gemini quota exhaustion (GH#57)
- chore: add GitHub issue templates (bug report + feature request)
- v8.39.0: Fix stats panic (GH#52), zombie tasks (GH#53), pending dispatch (GH#54)
- fix batch slot refill latency for pending tasks
- Auto-fail stale running tasks after 24h
- Fix stats panic on zero-duration tasks
- docs: update CLAUDE.md with v8.38.0 features (worktree prune, context sync)
- v8.38.0: Worktree context sync (GH#51), worktree prune, batch/background splits
- refactor: split batch serde and interpolation helpers
- Refactor background process and spec helpers
- Sync missing context files into worktrees
- Add stale worktree prune command
- v8.37.0: Code health + UX — run.rs split, ETA, quota-aware scheduling, auto-commit
- refactor(run): extract post-run lifecycle flow
- Prevent dispatching rate-limited agents
- Add ETA estimates for running board tasks
- improve merge auto-commit messages and staging
- docs: update CLAUDE.md with v8.36.0 features
- v8.36.0: Stats dashboard, merge target, tool team, Cargo.lock sync, 4 bug fixes
- Support comma-separated batch fallback agents
- Treat 402 payment errors as fallback eligible
- Accept string values for batch list fields
- fix(run): restore full output fallback from logs
- add aid stats command
- Add target branch support to aid merge
- fix merge Cargo.lock drift before worktree merge
- Add team lookup to tool show and test
- v8.35.0: Composer-2 default, output fix, batch fallback, agent config
- Add per-agent default model configuration
- Set Cursor composer-2 as default model
- Fix batch auto fallback agent selection
- fix(show-output): merge cursor assistant deltas
- fix: VFAIL keeps Done status — stop downgrading to Failed
- v8.34.0: Auto-sequence shared-worktree batch tasks + prompt size warning
- Auto-sequence shared batch worktrees
- Add team toolbox: configurable tools injected into agent prompts
- chore: auto-commit agent changes before merge
- Remove duplicate ToolAction enum, use cli_actions::ToolAction in tool.rs
- task A
- release: v8.32.0 — Python verify auto-detection
- Add Python verify auto-detection
- release: v8.31.1 — default TUI, foreground task visibility
- fix: sort task IDs in TUI group filter test for deterministic ordering
- fix: keep foreground tui tasks visible under group filter
- Default bare aid to board
- chore: remove accidentally committed build artifacts
- task A
- release: v8.31.0 — verify enforcement, quota rescue, rate limit quality, spawn logging, pending timeout
- fix: update verify retry test for enforce_verify_status behavior
- chore: auto-commit agent changes before merge
- fix: timeout stale pending tasks
- Fix: tasks that passed verify but failed due to quota exhaus
- fix: fail tasks when verify fails without retry
- Fix: when agent process fails to spawn, write an error event
- fix: clean saved rate limit markers
- release: v8.30.1 — batch [defaults] group support, close #42 & #33
- fix: support group field in batch [defaults] for workgroup assignment (#42)
- fix: add oz & droid to setup agent detection and rate_limit list
- release: v8.30.0 — Web UI v2
- Add web task action and diff endpoints
- Add task detail actions and diff tab
- release: v8.29.3 — code health round 3
- refactor: code health round 3 — extract run/prompt tests and helpers
- Extract the `#[cfg(test)]` test block from `src/cmd/run_prom
- Extract run command tests into run_tests module
- task B
- task A
- release: v8.29.1 — batch workgroup override (GH#40)
- Add batch workgroup override flag
- release: v8.29.0 — merge safety & batch analysis
- Add merge check mode and post-merge group verify
- add batch file overlap analysis
- release: v8.28.2 — fix output file enforcement (GH#37, GH#39)
- Fix output post-processing fallbacks
- release: v8.28.1 — dev environment container mode
- Add reusable dev container execution mode
- release: v8.28.0 — shared batch directory + changelog fix
- Add shared batch directory support
- docs: update README — version badge, new agents, sandbox section
- fix: aid changelog no longer shows other repo's tags
- fix: panic on multi-byte chars in prompt preview truncation
- release: v8.27.2 — code health round 2: split cli, config, watcher
- split cli command definitions into modules
- split watcher helpers into focused modules
- refactor(config): split config command modules
- release: v8.27.1 — code health: split 3 oversized files
- Split src/main.rs (739 lines) by extracting the command disp
- Split CLI command dispatch out of main
- Split src/cmd/show_output.rs (836 lines) into focused format
- Split aid show output formatters into focused modules
- Split src/store/queries.rs (807 lines) into focused query mo
- split store query modules
- fix: include updated Cargo.lock for v8.27.0
- release: v8.27.0 — container sandbox for agent isolation
- - [Review Checklist](knowledge/review-checklist.md) — Pre-ac
- feat: add container sandbox run option
- release: v8.26.1 — single source of truth for agent lists
- refactor: derive charts AGENTS from AgentKind::ALL_BUILTIN
- - [Coding Conventions](coding-conventions.md) — File structu
- Unify built-in agent metadata on AgentKind
- release: v8.26.0 — skill scripts with structured metadata
- Add script metadata parsing and structured injection to the
- Add skill script metadata injection
- release: v8.25.1 — fix 4 GitHub issues (#30-#35)
- Fix GH#34: opencode crashes when sibling task context is inj
- Fix GH#31: `read_only = true` tasks should NOT auto-commit a
- Fix GH#32: Warn when multiple batch tasks target the same `d
- task A
- task A
- task A
- chore: auto-commit agent changes before merge
- - [Coding Conventions](coding-conventions.md) — File structu
- fix: correct test indentation in stop.rs
- Fix `aid stop` and zombie detection to properly kill agent p
- Fix the `aid changelog` command in src/cmd/changelog.rs and
- Fix process leaking in the PTY bridge. When a PTY-spawned ag
- release: v8.24.1 — changelog anywhere + cursor log cleanup
- fix: add build.rs for embedded changelog
- release: v8.24.0 — batch & UX polish
- Add two new flags to `aid show`:
- Add summary and file filters to aid show
- Fix auto-commit failing on empty git repos (repos with no HE
- Handle auto-commit in repos without HEAD
- Make `aid watch --quiet` less verbose by suppressing milesto
- Suppress quiet wait milestone progress output
- Change droid's default auto approval level from "medium" to
- agent: raise droid auto approval to high
- release: v8.23.0 — skill system v2 with folders, gotchas, and scripts
- fix: add missing test files from skill folder worktree
- Upgrade the skill system to support folder-based skills with
- release: v8.22.1 — add aid changelog command
- Add changelog subcommand for release history
- release: v8.22.0 — batch power-ups & cost visibility
- Add `.env` forwarding to agent subprocesses.
- Add synthetic progress events for droid agent (and any agent
- Add aid cost reporting command
- Add batch template variable interpolation
- Preserve partial work on retry by default
- release: v8.21.14 — custom agent docs clarification
- fix: clarify custom agents are non-interactive CLIs, not Claude Code
- feat: add --full flag to show --output and aid output
- chore: update Cargo.lock for v8.21.12
- fix: auto-commit message uses [Task] section instead of shared context
- release: v8.21.12 — performance + test subprocess leak fix
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- release: v8.21.11 — fix GH#22 gemini tool_call name parsing
- fix: GH#22 gemini tool calls logged as 'unknown'
- release: v8.21.10 — security hardening from core audit
- fix: security hardening from core audit — 4 HIGH findings resolved
- release: v8.21.9 — zombie detection false positive fix
- fix: zombie detection false positives — waitpid ECHILD for non-child workers
- Auto-created for batch dispatch
- - [Agent System](agent-system.md) — Selection pipeline, prom
- chore: update Cargo.lock for v8.21.8
- refactor: ProcessGuard RAII subprocess abstraction + verify.rs migration
- fix: GH#27 droid/codebuff rejected in batch — replace hardcoded VALID_AGENTS with AgentKind::parse_str
- release: v8.21.6 — subprocess management perf fixes
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- release: v8.21.5 — eprintln to aid output macros bulk conversion
- fix: GH#25 remove cursor from auto-skills, GH#26 batch auto-cascade for rate-limited agents
- fix: agent subprocess leak — process group isolation for all spawn paths
- Auto-created for batch dispatch
- Auto-created for batch dispatch
- fix: GH#22 gemini tool names, GH#23 auto-create group, GH#24 0B output, GH#28 judge bool
- release: v8.21.1 — fix verify process leak (GH#27)
- fix: verify process leak — process group isolation + timeout (GH#27)
- release: v8.21.0 — attention space audit + quiet mode + droid parity
- release: v8.20.9 — show-output extraction, verify isolation, auto-commit cleanup
- fix: auto-commit uses git add -u instead of -A, skips context headers in message
- fix: show-output extraction, verify isolation, batch audit safety, retry reset
- release: v8.20.8 — code health cleanup
- chore: extract inline tests to separate files — merge, selection, watcher
- chore: remove last production unwrap() in usage.rs
- chore: dead code cleanup — remove 4 dead items, 10 unnecessary annotations
- release: v8.20.7 — context_from implicit dependencies + unwrap safety
- fix: context_from creates implicit dependency in batch dispatch
- release: v8.20.6 — zero production unwrap()
- fix: remove all unwrap() from production code paths
- release: v8.20.5 — data integrity fixes
- fix: data integrity — auto-commit error events + workgroup creation rollback
- release: v8.20.4 — zero clippy warnings
- fix: eliminate all clippy warnings (11 → 0)
- release: v8.20.3 — propagate workgroup env to agent subprocesses
- fix: propagate AID_GROUP and AID_TASK_ID to agent subprocesses (#15)
- release: v8.20.2 — --dir agent isolation via GIT_CEILING_DIRECTORIES
- fix: set GIT_CEILING_DIRECTORIES to prevent --dir agent escape (#16)
- release: v8.20.1 — subprocess concurrency limits
- feat: subprocess concurrency limits for tests and runtime
- feat: [Shared Context: batch] Auto-created for batch dispatch
- feat: [Shared Context: batch] Auto-created for batch dispatch
- feat: [Shared Context: batch] Auto-created for batch dispatch
- release: v8.20.0 — Droid (Factory.ai) agent integration
- chore: auto-commit agent changes before merge
- release: v8.19.0 — agent quota + structured findings
- chore: auto-commit agent changes before merge
- feat: [Team Knowledge — ai-dispatch] - [Coding Conventions](coding
- fix: pass --cascade through BackgroundRunSpec (closes #17)
- chore: auto-commit agent changes before merge
- release: v8.18.0 — process safety, idle timeout & double-dispatch fix
- feat: v8.18.0 — process safety, idle timeout, double-dispatch fix
- release: v8.17.2 — commit message sanitization + zero warnings
- fix: strip aid tags from auto-commit messages + eliminate compiler warnings
- release: v8.17.1 — process management audit fix
- fix: reap on_done callback children to prevent process leak
- release: v8.17.0 — batch resilience + process safety
- feat: batch resilience, performance tuning, process group safety (v8.17.0)
- release: v8.16.0 — comprehensive security hardening
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- feat: <aid-team-rules> - Do NOT run cargo fmt, rustfmt, or any aut
- Harden worktree cleanup and branch reset safety
- feat: <aid-team-rules> - Do NOT run cargo fmt, rustfmt, or any aut
- feat: add sanitize module — input validation + path safety layer
- release: v8.15.2 — defense-in-depth sandbox guards + docs update
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- release: v8.15.1 — critical worktree sandbox guard
- fix: sandbox guard for worktree cleanup — prevent data loss
- release: v8.15.0 — local web UI dashboard
- feat: local web UI dashboard + batch init + show anti-polling (v8.15.0)
- release: v8.14.1 — code quality audit cleanup
- refactor: code quality audit — simplify error handling, fix fragile matching
- release: v8.14.0 — project init guidance, failure reasons, cursor-agent detection
- feat: CLAUDE.md emphasizes aid as primary dev method, session-start hints project init
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- fix: show failure reason in CLI output, detect cursor-agent binary, remove TUI hint
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- release: v8.13.0 — cursor agent overhaul, TUI failure reasons
- fix: cursor agent overhaul — standalone binary, event parsing, TUI failure reasons
- fix: correct install URL to aid.agent-tools.org
- ci: fix release workflow — use macos-15 runner (macos-13 deprecated)
- release: v8.12.0 — GitHub issues sprint, CI, repo cleanup
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- fix: remove unused imports in upgrade.rs for Linux clippy
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- fix: add #[cfg(target_os = "macos")] to home_cargo_bin to fix clippy on Linux CI
- ci: build release binary instead of just cargo check
- ci: add CI workflow for push/PR — cargo check, test, clippy
- Revert "feat: <aid-project-rules>"
- feat: <aid-project-rules> - File size limit: 300 lines per file -
- Reuse batch default workgroups when present
- Add stdin and file input for findings
- feat: [Team Knowledge — dev] - [Review Checklist](knowledge/review
- chore: move batch TOMLs to .aid/batches/, gitignore that directory
- chore: repo cleanup — remove stale batch TOMLs, nanobanana-output, website dirs
- ci: add GitHub Release workflow with cross-compiled binaries
- release: v8.11.0 — prompt hardening, UX improvements, commit pollution fix
- feat: UX improvements + fix commit message pollution
- chore: remove batch dispatch file
- fix: harden prompt injection pipeline against cross-task pollution
- release: v8.10.0 — configurable pricing + command consolidation
- feat: configurable pricing + command consolidation
- fix: install script now shows aid setup + aid init next steps
- feat: add /api/pricing endpoint and fix model prices
- website: replace agent matrix with positioning cards
- docs: remove ob1 references from README
- docs: update README and website to v8.9.1
- release: v8.9.1 — caller-controlled hiboss notifications
- fix: remove auto hiboss notifications, caller-controlled only
- release: v8.9.0 — interactive approval + batch organization
- feat: hiboss Layer 1 rich notifications (v8.8.0)
- release: v8.7.1 — auto-dir + background quota cascade
- fix: improve batch help — show [defaults] fields including dir
- release: v8.7.0 — reliability & cost control
- docs: update CLAUDE.md with v8.6 project features
- release: v8.6.0 — project & budget UX overhaul
- Add project sync command
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-team-rules> - Do NOT run cargo fmt, rustfmt, or any aut
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- release: v8.5.3 — code quality + UX fixes
- fix: warn when merging VFAIL tasks
- fix: --context and --scope accept space-separated values
- chore: zero clippy warnings (15 fixed across 10 files)
- release: v8.5.2 — knowledge injection quality improvements
- fix: improve knowledge injection quality — filter threshold, stop words, dedup, truncation
- chore: auto-commit agent changes before merge
- release: v8.5.1 — auto-stash merge + milestone prompt fix
- fix: auto-stash local changes before merge + clarify milestone prompt
- chore: auto-commit agent changes before merge
- chore: populate project knowledge base with 5 entries
- chore: update aid-website to v8.5.0 — add project profiles, project command
- docs: add project profiles to README, CLAUDE.md, claude-prompt.md
- release: v8.5.0 — project profiles (.aid/project.toml)
- chore: suppress dead_code warnings for ProjectConfig/ProjectAgents schema fields
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- feat: <aid-system-context> [Shared Workspace] Path: /tmp/aid-wg-wg
- chore: add mod project to main.rs
- chore: auto-commit agent changes before merge
- release: v8.4.0 — agent UX guardrails + team rules
- feat: team rules — always-injected behavioral constraints
- fix: UX improvements — parse hint, workspace tag, reuse test canonicalize
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f624 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f624 Use this direct
- fix: replace global Mutex with thread_local for test isolation
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78d1 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78d1 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78d1 Use this direct
- feat: hiboss notification channel + fix --id FK constraint
- docs: update website for v8.3.0 — stop, kill, steer commands
- v8.3.0: Live Task Control — stop, kill, steer
- v8.2.0: Custom IDs, Cursor CLI Upgrade, Work Scope Verification
- v8.1.0: Model-Level Scoring, Task Pre-creation, Rate Limit Auto-clear
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-aae8 Use this direct
- v8.0.0: Programmable Orchestration — validation, structured diff, loop detection
- v7.9.1: binary size 67% reduction + SQLite index optimization
- perf: add SQLite indexes on hot query paths + fix compiler warnings
- perf: add release profile — strip + LTO + codegen-units=1
- refactor: replace ureq with curl subprocess, drop rustls dependency
- v7.9.0: Code Health — file splits + milestone strip
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bc39 Use this direct
- feat: improved TUI tree view — workgroup grouping, navigation, live status
- perf: TUI performance optimization — batch queries + throttled metrics
- release: v7.8.0 — Autonomous Experiment Loop + TUI Tree View
- feat: add experiment loop core + CLI wiring
- feat: add rolling context compression for workgroup prompts
- feat: add tree view mode to TUI (toggle with 't' key)
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-54ea Use this direct
- fix: get_completion_summary NULL handling + experiment status/persist wiring
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-ca3d Use this direct
- release: v7.7.0 — Collective Intelligence
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-a6ea Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-a6ea Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-a6ea Use this direct
- release: v7.6.0 — Shared Context Threads
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-c886 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-c886 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-c886 Use this direct
- chore: remove dispatch batch TOMLs from repo, update gitignore
- release: v7.5.2 — stabilization (zero clippy warnings, SQL fix, 295 tests)
- feat: Fix ALL clippy warnings in the codebase. Run `cargo clippy -
- fix: include merged status in similar-tasks query, align batch test fields
- fix: robust judge parsing, diff truncation, committed-diff support
- release: v7.5.1 — memory quality + dispatch intelligence
- feat: surprise-filter, cross-session hints, best-of-n dispatch (v7.5 P1)
- release: v7.5.0 — routing intelligence (budget-aware routing + auto-judge)
- feat: auto-judge review + budget-aware cost-efficiency routing (v7.5)
- feat: budget-aware cost-efficiency routing for agent auto-selection
- release: v7.4.0 — episodic memory, success routing, code health
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6c91 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6c91 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6c91 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6085 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6085 Use this direct
- fix: add --events flag to aid show (no-op, documents default behavior)
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-6085 Use this direct
- fix: run merge verify command through shell for redirect support
- release: v7.3.0 — code health, file splits, batch UX
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- fix: accept both [[task]] and [[tasks]] in batch TOML files
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-2f03 Use this direct
- release: v7.2.2 — retry --dir override, fast-fail diagnostic hint
- release: v7.2.1 — fix streaming -o, remove OB1 agent
- fix: write output file for streaming agents (-o flag)
- release: v7.2.0 — model cascade, conditional batch chains
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f652 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-f652 Use this direct
- release: v7.1.0 — empty diff guard, foreground timeout, zero warnings
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78b3 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78b3 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-78b3 Use this direct
- release: v7.0.1 — retry worktree reuse, exit_code in JSON output
- fix: retry reuses existing worktree, exit_code in --json output
- fix: rename task_hook_json to avoid duplicate definition after merge
- feat: v7.0 foundation — JSON output, result forwarding, workspace, trust tiers
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- feat: [Shared Workspace] Path: /tmp/aid-wg-wg-bd59 Use this direct
- release: v6.1.0 — teams as knowledge context, not agent restrictions
- feat: teams as knowledge context — soft preferences, not agent restrictions
- docs: update website for v6.0.0 — add Teams section, team command, version bump
- release: v6.0.1 — improved UX for in-place tasks
- fix: improve UX for in-place (no worktree) tasks
- feat: aid team — native team concept for role-based agent selection
- release: v5.9.2 — merge-group test + real-world merge validation
- chore: auto-commit agent changes before merge
- release: v5.9.1 — fix merge data-loss, comprehensive merge tests
- test: comprehensive merge tests — 17 new tests covering all data-loss scenarios
- fix: prevent data loss in aid merge — validate commits, auto-commit, proper cleanup
- chore: v5.9.0 — store v2 versioning, skill packages, graceful upgrade
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v59-features] Auto-created for batch dispat
- chore: bump version to 5.8.2
- fix: improve show --diff and merge UX for non-worktree tasks
- chore: v5.8.1 — update README, website docs for fast query & setup
- fix: setup differentiates first-time vs returning users
- fix: setup shows current config status when already configured
- fix: setup wizard UI polish — sections, key masking, verify spinner
- feat: setup detects all built-in agents + custom agents
- fix: setup wizard shows "Press Enter to skip" hint
- feat: aid setup — interactive configuration wizard
- fix: default free tier to openrouter/free
- feat: v5.8.0 — aid query (fast LLM via OpenRouter)
- feat: auto-publish to crates.io on tag push + install.sh
- fix: strip com.apple.provenance xattr in install command
- chore: v5.7.0 — broadcast bridge, false-positive fix, workspace setup
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v57-broadcast] Auto-created for batch dispa
- feat: [Shared Context: v57-broadcast] Auto-created for batch dispa
- docs: update README and website for v5.4-5.6 features
- docs: add project CLAUDE.md with install instructions
- chore: v5.6.1 — CLI arg ergonomics (group create optional context, summary positional group, run -g)
- fix: improve CLI arg ergonomics
- chore: v5.6.0 — shared findings for workgroup collaboration
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: [Shared Context: v56-findings] Auto-created for batch dispat
- feat: v5.5.0 — task tree visualization, workgroup summary
- feat: [Shared Context: v55-tree-summary] Auto-created for batch di
- feat: [Shared Context: v55-tree-summary] Auto-created for batch di
- feat: [Shared Context: v55-tree-summary] Auto-created for batch di
- chore: v5.4.2 — orchestrator-only memory, explicit --project flag
- feat: memory update command + age in prompt injection
- fix: memory list/search project-scoped by default, add --all flag
- fix: memory list/search auto-scopes to current project
- chore: v5.4.1 — bug fixes, task export, dogfood improvements
- fix: update auto-retry test for verify_status behavior change
- fix: revert unnecessary load_metrics expansion to completed tasks
- fix: TUI Progress column shows milestones for completed tasks
- feat: [Shared Context: v54-fixes-and-export] Auto-created for batc
- fix: verify failure should not override task status to Failed
- feat: [Shared Context: v54-fixes-and-export] Auto-created for batc
- feat: [Shared Context: v54-fixes-and-export] Auto-created for batc
- chore: v5.4.0 — agent memory system, verify status
- feat: add VerifyStatus to distinguish execution failure from verify failure
- fix: align memory CLI with canonical Memory struct
- feat: add aid memory CLI commands
- feat: add memory injection to prompt pipeline
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: [Shared Context: v54-memory] Auto-created for batch dispatch
- feat: add agent store website at store.agent-tools.org
- chore: v5.3.1 — migrate agent store to agent-tools-org, add script support
- chore: migrate repo to agent-tools-org organization
- docs: update README and website for v5.2-5.3 features
- chore: v5.3.0 — hooks, prompt compaction, UTF-8 safety
- fix: UTF-8 safe truncation + hooks test constructors
- fix: align indentation in main.rs hooks wiring
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v53-hooks-compaction] Auto-created for batc
- chore: v5.2.0 — agent analytics, agent fork, test deadlock fix
- feat: [Shared Context: v52-features] Auto-created for batch dispat
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v52-features] Auto-created for batch dispat
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v51-release] Auto-created for batch dispatc
- chore: bump version to 5.1.0
- feat: [Shared Context: v51-store-wave2] Auto-created for batch dis
- fix: custom agent display name + background worker + retry resolution
- feat: add aid store subcommand (browse, install, show)
- fix: use correct custom agent TOML fields (id + display_name)
- fix: escape AID_TASK_ID in custom agent example to fix tsc
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to 5.0.1
- fix: v5.0.1 — custom agent dogfood fixes + contention prevention
- feat: [Shared Context: v50-contention] Auto-created for batch disp
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v50-dogfood] Auto-created for batch dispatc
- feat: [Shared Context: v50-dogfood] Auto-created for batch dispatc
- feat: [Shared Context: v50-dogfood] Auto-created for batch dispatc
- feat: v5.0 — custom agent definitions, agent CLI, worktree base branch fix
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: [Shared Context: v50-wave1] Auto-created for batch dispatch
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: add agent-optimized website at aid.agent-tools.org
- feat: v4.8 — stabilization: codebuff cost, worktree escape, TUI dim
- feat: [Shared Context: v48-bugs] Auto-created for batch dispatch
- feat: IMPORTANT: When editing text/config files, make targeted ed
- docs: update README for v4.7 — codebuff setup guide, cost warning, pricing update
- feat: v4.7 — self-evaluation fixes, pricing update, codebuff cost tracking
- chore: bump version to v4.7.0
- feat: v4.6 — cost tracking overhaul, agent-aware cost labels
- feat: [Context] [Context Files - read these before starting] - src
- fix: upgrade codebuff SDK to v0.10 — local agent execution, no WebSocket
- feat: v4.5 — codebuff plugin, TUI stats view, retry worktree fix
- feat: [Context] [Context Files - read these before starting] - src
- feat: v4.4 — intelligent task routing with classifier + capability matrix
- fix: word-boundary matching for classifier, poison-safe AidHomeGuard
- feat: [Context] [Context Files - read these before starting] - src
- chore: bump version to v4.3.0
- feat: v4.3 — ob1 coding support, cursor budget model, startup zombie cleanup
- feat: [Shared Context: v43-fixes] Auto-created for batch dispatch
- docs: update README for v4.2 — ob1 agent, worktree CLI, workspace isolation
- chore: add ob1 to available agents list in error message
- feat: add ob1 agent adapter — multi-model coding CLI
- fix: worktree list handles macOS /private/tmp symlink
- feat: add `aid worktree create/list/remove` CLI commands
- feat: worktree escape detection — warn if agent modified main repo
- fix: watch --group scope leak, auto cherry-pick on merge
- chore: bump version to v4.1.0
- refactor: split TUI modules — app.rs and ui.rs under 300-line limit
- feat: workspace isolation — AID_GROUP env var, auto-cleanup, merge precheck
- feat: upgrade agent capabilities — cursor/gemini coding support, fallback chain
- chore: bump version to v4.0.1
- feat: progress reporting in quiet watch + board poll detection
- fix: TUI color palette — fix invisible selected text, improve contrast
- docs: update README for v4.0 — clean, merge --group, CLI hints
- chore: bump version to v4.0.0
- feat: aid merge --group for bulk merging workgroup tasks
- feat: watch hints after background dispatch and batch
- feat: contextual CLI hints and after_help examples
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.9.0
- fix: auto-retry after verify failures
- feat: TUI detail view tab system — events/prompt/output
- feat: [Shared Context: v39-wave2] Auto-created for batch dispatch
- feat: [Shared Context: v39-wave2] Auto-created for batch dispatch
- feat(batch): support defaults section
- feat: [Shared Context: v39-wave1] Auto-created for batch dispatch
- docs: update README for v3.8 — stream board, batch fields, kilo agent
- chore: bump version to v3.8.0
- feat: v3.8 — modular architecture, stream board, TUI polish
- feat: batch read_only/budget fields, auto-budget detection, TUI duration fix
- chore: bump version to v3.7.0
- feat: v3.7 — rate-limit auto-expiry, batch pre-check, worktree lock fix
- feat: [Shared Context: v37-tasks] Auto-created for batch dispatch
- feat: [Shared Context: v37-tasks] Auto-created for batch dispatch
- feat: [Shared Context: v37-tasks] Auto-created for batch dispatch
- chore: bump version to v3.6.0
- feat: clear-limit CLI, codex model passthrough, gpt-5.4 registry
- chore: bump version to v3.5.1
- feat: TUI multipane v2 — scrolling, rich headers, all tasks, Enter/Esc navigation
- chore: bump version to v3.5.0
- feat: enrich TUI multipane with duration, tokens, cost, model, milestone, metrics
- feat: batch verify=true support, rate-limit precheck, diff exclude locks, CLI help
- chore: bump version to v3.4.0
- feat: model-level history stats, budget model auto-selection, improved CLI help
- feat: [Shared Context: v34-wave1] Auto-created for batch dispatch
- feat: [Shared Context: v34-wave1] Auto-created for batch dispatch
- feat: [Shared Context: v34-wave1] Auto-created for batch dispatch
- chore: bump version to v3.3.0
- feat: multi-task watch support and indent fix
- enhance rate-limit tracking to store recovery time and display in config
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.2.0
- fix: align multipane bridge with structured PaneData events
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.1.0
- feat: add --exit-on-await flag for manager notification
- fix: add Kilo to agent usage stats iteration
- feat: add history-based agent scoring for auto-selection
- feat: IMPORTANT: When editing text/config files, make targeted ed
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: bump version to v3.0.0
- docs: add kilo to agent help text
- feat: IMPORTANT: When editing text/config files, make targeted ed
- fix: disable prompt detection for streaming agents
- chore: remove batch dispatch files
- chore: bump version to v2.9.0
- feat: add OpenCode --session retry for session continuity
- fix: add missing agent_session_id to test Task structs
- feat: pass context files to OpenCode via -f flag
- feat: IMPORTANT: When editing text/config files, make targeted ed
- chore: rename crate to ai-dispatch for crates.io publish
- chore: bump version to v2.8.0
- feat(retry): add --agent flag to override agent for retries
- feat: [Shared Context: v28-resilience] Auto-created for batch disp
- feat: add text-edit prompt guard for non-code files
- feat: sync Cargo.lock toworktrees to avoid redundant dependency resolution
- feat: validate fallback agent in batch file parser
- chore: bump version to v2.7.0
- feat: [Shared Context: v27-native-flags] Auto-created for batch di
- feat(gemini): upgrade to streaming mode with native CLI flags
- feat: [Shared Context: v27-native-flags] Auto-created for batch di
- feat: use native CLI flags for read-only and full-auto modes
- feat: [Shared Context: v27-native-flags] Auto-created for batch di
- chore: bump version to v2.6.0
- feat: [Shared Context: v26-efficiency-opencode] Auto-created for b
- feat: add auto rate-limit detection for codex
- chore: bump version to v2.5.0
- feat: [Shared Context: v25-polish] Auto-created for batch dispatch
- feat: [Shared Context: v25-polish] Auto-created for batch dispatch
- fix: parse OpenCode JSON token events
- fix(cursor): parse stream-json token usage
- feat: [Shared Context: v25-polish] Auto-created for batch dispatch
- feat: add --fallback agent and fix codex worktree trust
- feat: add `aid init` command with default skills
- chore: prepare for open source release
- chore: bump version to v2.2.0
- feat: [Shared Context: v22-budget] Auto-created for batch dispatch
- feat: add budget-aware agent selection
- feat: [Shared Context: v22-budget] Auto-created for batch dispatch
- fix(show): fall back to default log output
- chore: bump version to v2.1.0
- fix(board): show awaiting prompt instead of output context
- feat: add completion notification feed
- feat(respond): accept stdin and file input
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- fix(batch): persist skipped dependency tasks
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: add show context prompt inspection
- feat(batch): limit concurrent batch dispatches
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- docs: update README for v2.0.0 and add Claude Code prompt file
- feat(cli): add merge command for completed tasks
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- feat: [Shared Context: v21-robustness] Auto-created for batch disp
- chore: bump version to v2.0.0
- feat: add multi-repo task dispatch
- feat(templates): add prompt template support
- feat: [Shared Context: v20-capabilities] Auto-created for batch di
- feat: [Shared Context: v20-capabilities] Auto-created for batch di
- feat: add task completion webhooks
- feat: add benchmark command for multi-agent comparisons
- docs: update README for v1.7.0 features
- chore: bump version to v1.7.0
- fix: inherit retry worktree base
- feat: show retry chain history
- feat: make task max duration configurable
- feat(usage): add per-agent execution stats
- feat: [Shared Context: v17-ux] Auto-created for batch dispatch
- feat: [Shared Context: v17-ux] Auto-created for batch dispatch
- feat: [Shared Context: v17-ux] Auto-created for batch dispatch
- chore: bump version to v1.6.0
- refactor(show): extract explain module
- refactor(cmd): extract retry logic from run
- feat: [Shared Context: v16-quality] Auto-created for batch dispatc
- feat: [Shared Context: v16-quality] Auto-created for batch dispatc
- feat: [Shared Context: v16-quality] Auto-created for batch dispatc
- docs: update README for v1.5.0 features
- chore: bump version to v1.5.0
- feat: [Shared Context: v15-fixes] Auto-created for batch dispatch
- feat: [Shared Context: v15-fixes] Auto-created for batch dispatch
- feat: [Shared Context: v15-fixes] Auto-created for batch dispatch
- feat: dependency-based DAG scheduling and v1.4.0 release
- feat: add agent capability profiles and pricing table
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- feat: [Shared Context: v14-features] Auto-created for batch dispat
- chore: release aid v1.3.0
- fix: detect zombie/defunct processes in zombie task cleanup
- feat: add skills parameter to aid_run MCP tool
- feat: enforce post-task worktree commits
- feat(tui): add dashboard view
- feat(run): auto-apply default skills
- chore: release aid v1.2.0
- fix: revert unintended README changes from interactive-io task
- feat: add PTY input forwarding for background tasks
- docs: rewrite ai-dispatch readme
- feat: share workgroup milestone findings
- chore: release aid v1.1.1 — milestone reporting
- feat: surface task milestones in dashboards
- chore: release aid v1.1.0
- feat: add skill injection for methodology-guided agent dispatch
- feat: add MCP server mode for native Claude Code tool calls
- feat: add smart agent auto-selection
- feat: add process metrics to tui dashboard
- feat: consolidate CLI from 17 to 11 commands for v1.0
- feat: fix 4 reliability bugs for v1.0
- chore: fix clippy warnings and bump to v0.9.0
- feat: add task dependency DAG to batch dispatch
- feat: add `aid explain` — AI-assisted task log explanation
- feat: scope tui watch by task and workgroup
- chore: release aid v0.8.0
- feat: add workgroup lifecycle commands
- chore: release aid v0.7.0
- feat: extend workgroup task views
- feat: add workgroup shared context
- chore: release aid v0.6.0
- feat: improve streaming usage tracking
- feat: add wait commands for task orchestration
- feat: release aid v0.5.0
- feat: v0.5 Phase 0 — command stubs, store migration, audit/review extraction
- chore: v0.5 foundation — add deps, Serialize derives, parent_task_id
- feat: v0.4 verify + context + review (agent collaboration)
- feat: v0.3 worktree isolation, batch dispatch, cursor adapter
- feat: v0.2 observability — cost tracking, OpenCode adapter, stderr capture, richer events
- feat: implement aid MVP v0.1 — multi-AI CLI team orchestrator
- Initial commit: add DESIGN.md

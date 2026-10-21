KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb verify false passed remote-test noop`.
Matches included “a harness that collected nothing must not grade”, “a failed status poll is not
a failed job”, and “a remote test gate that reports zero binaries is a transport failure”.

## Reproduction and captured facts

The read-only local database records t-8aa27628 as done, verify=auto, verify_status=passed,
effective_dir=~/.aid/worktrees/ai-dispatch-5b113037/feat/codex-provider-route,
final_head_sha=8462b7c60c4e138e0ee6ebef6ac228f69d9b0906. Saved dispatch_args has verify=auto,
dir=., worktree=feat/codex-provider-route, and a named remote build box. Four noop events exist.
The supplied test-compilation failure is not independently rerun: the brief prohibits compilation.

CONFIRMED: explicit auto bypasses the project verify default in dispatch_resolve.rs;
maybe_verify_impl translates auto to None; the named remote-box path uses task::verify::auto_detect_command,
which chooses cargo check for Rust.
Without a saved box, BuildRequest::for_verify also chooses cargo check.
The generic task verifier also auto-detects cargo check. Thus a passing check does not imply that
the bin test target compiles. The configured scripts/remote-test.sh command was not selected by
this path. NoOp is not a verification short circuit: nothing_to_verify_reason only checks read_only.

AMBIGUOUS: the historical command output, observed process exit code, remote checkout HEAD and
sync payload were not persisted. The expected command is cargo check (through the saved-box PATH shim);
reconstructing it is not evidence of its actual remote execution. The local effective
directory and final HEAD are recorded above, but do not prove the tested remote tree. No evidence
supports a lost script exit status: the script was bypassed by command selection.

CONFIRMED: maybe_verify_impl prints its report and records status; successful verification creates
no event. Background spawn redirects worker stdout to /dev/null (spawn.rs redirect_worker_stdio),
while worker.log receives stderr. This explains an empty log and absent successful verify event.

CONFIRMED: CodexAgent::parse_event searches the entire JSON stream line for NO_CHANGES_NEEDED
before inspecting item type. Tool command/output literals therefore become NoOp events. NoOp has
no direct lifecycle or verify consumer; EventKind::is_liveness excludes it and foreground
watching excludes it from live-event hints. Delivery inspection remains based on workspace facts.

## Evidence locations and candidate assessment

- Historical tree 8462b7c6: src/cmd/run/dispatch_project_defaults.rs:18 only applies the
  configured project command when args.verify is None. Saved auto survives unchanged.
- Current equivalent src/cmd/run/dispatch_resolve.rs:57 now treats auto as requesting the
  project default. Explicit non-auto commands still win; no project retains auto-detection.
- src/cmd/run/verify.rs:34 maps remaining auto to None; lifecycle/steps.rs:69 supplies
  effective_dir, not caller cwd. src/runtime/remote_build/mod.rs:135 selects the saved-box
  path; :173 invokes run_verify_with_env with that path and the saved box environment.
- src/task/verify/mod.rs:205 detects cargo check. Its :126 and :135 derive exit_code and
  success from the actual process ExitStatus. A recorded Passed means success=true reached
  record_verify_status, not that the configured remote-test script completed.
- src/runtime/remote_build/cargo.sh:5-17 derives the sync root from the command's checkout,
  not the canonical main checkout. The predicted remote directory is
  ~/.rbox/work/ai-dispatch/feat-codex-provider-route. Its actual synced HEAD is unknown.
- tasks.final_head_sha is delivery identity, not remote verification identity. The row's
  exit_code=NULL is the agent outcome field, not captured verifier evidence.
- Event IDs 259713, 259794, 260082, 260162 are noop rows; the read-only query found zero
  events with a Verify/Verification detail. worker.log was reported empty in the brief.
- src/runtime/background/spawn.rs:100-108 redirects stdout to /dev/null; :25 routes stderr
  to worker.log. src/cmd/run/verify_outcome.rs now holds the original stdout report.
- src/agent/codex/events.rs:51 now checks completed agent_message text, final_answer phase
  (or absent phase for CLI messages), and a leading marker. Tool output and reasoning do
  not qualify. The app-server normalizer preserves item phase when converting agentMessage.
- NoOp neither sets verify_status nor skips configured verification. Its direct behavioral
  consumer is liveness filtering (src/types/status.rs:200), plus foreground display filtering.
  The false NoOp rows are independent of the verification-command selection defect.

Different-checkout and lost-script-exit theories are not established. The saved auto value and
Rust auto-detection explain the apparent contradiction without either. Historical command,
remote tree and numeric verifier exit cannot be asserted as observed facts from this database.

## Changes and regression coverage

1. Resolve auto to the configured project verifier at dispatch, before task arguments are saved.
   tests/verify_shell_e2e.rs::auto_verify_uses_project_command_instead_of_rust_check uses a
   Rust-looking checkout and a project verifier returning 101, checks task failure, and checks
   the saved concrete command. dispatch_verify_tests.rs adds a command-precedence matrix.
   Before the fix, auto remains persisted and this regression fails.
2. src/cmd/run/verify_evidence.rs records started and finished events independently of stdout.
   Finished metadata includes resolved command, directory, local HEAD before/after, exit code,
   timeout, success, and a bounded output excerpt. Remote tested HEAD is explicitly unknown.
   Persistence errors propagate as infrastructure failures before Passed can be recorded.
   Existing outcome classification was moved into verify_outcome.rs without changing its rules.
   background_verify_persists_success_and_failure_evidence exercises detached runs returning
   0 and 101, and asserts durable command/directory/HEAD/exit/output metadata. Before the fix,
   neither run has a finished verification event.
3. Codex NoOp parsing moves from raw JSON substring matching into completed agent-message
   handling. noop_only_comes_from_completed_agent_delivery rejects command text/output,
   partial messages, commentary and reasoning, and accepts a final delivery marker. Before
   the fix the tool fixtures become NoOp. CLI help and the dispatch guide describe precedence
   and durable evidence.

## Validation and limits

Checkout base HEAD: 41fb709922f26d4abaa98fcbdc1fe0f85fc8cb84. No remote tested HEAD:
compilation, cargo, aid build/test and remote E2E execution were prohibited by the brief.
The new Rust regressions are written but unrun; their expected before/after behavior above
is derived from the code, not a claim of executed test results.

Commands executed for validation:

- `python3 scripts/remote-test-test.py`: 19 passed, 0 failed; fake rbox only, no compilation.
  These existing tests check shell argument fidelity, original status preservation (including
  completed 75/124), zero-match handling and guide contracts; they do not execute the Rust fix.
- `python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py
  default-skills/aid-guide`:
  blocked before validation, `ModuleNotFoundError: No module named 'yaml'`. No environment workaround.
- `rustfmt --edition 2024 --config skip_children=true src/cmd/run/verify_evidence.rs
  src/cmd/run/verify_outcome.rs tests/verify_shell_e2e.rs`;
  `rustfmt --edition 2024 --config skip_children=true tests/verify_shell_e2e.rs
src/cmd/run/dispatch_verify_tests.rs`:
  passed. Existing formatting outside appended test/helper code was retained.
- `rustfmt --edition 2024 --config skip_children=true --emit stdout src/agent/codex.rs
  src/agent/codex/events.rs src/agent/codex_tests.rs src/cli/run_args.rs src/cmd/run/dispatch_resolve.rs
  src/cmd/run/dispatch_verify_tests.rs src/cmd/run/verify.rs src/cmd/run/verify_evidence.rs
  src/cmd/run/verify_outcome.rs tests/verify_shell_e2e.rs > /dev/null`:
  parsing passed; this is not type checking.
- `git diff --check`: passed.

Required external verification after commit: `aid test --bin aid
noop_only_comes_from_completed_agent_delivery`,
`aid test --bin aid auto_verify_prefers_project_command_and_preserves_explicit_commands`,
`aid test --test verify_shell_e2e`, plus the documented official-guide suites. Run through the
operator's authorized remote gate; no remote result is claimed here.

Choices: resolve project precedence once at dispatch to persist a stable concrete command;
use ordinary milestone events with structured verification metadata rather than a schema or
new event-kind migration; retain local and remote HEAD identities separately. Old task records
are not rewritten. Unconfigured Rust auto-detection still runs cargo check. Claude's separate
raw-marker parser was observed but left unchanged because the brief names Codex. No side tasks
were dispatched. Independent cross-review remains outstanding under the no-dispatch constraint.

- `security-guard --repo . staged`: passed, zero findings.

## Round 1 follow-up: settlement observation and final-message markers

KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb verification terminal status settlement evidence`.
Relevant matches included “a negative control can fail for its own reason” and
“a flag-selected write path makes a new field null on half the fleet”.

Operator-provided remote result on 5345c674: 3753 passed, 1 failed; later targets did not run.
The failing query in background_verify_persists_success_and_failure_evidence returned no rows.
CONFIRMED in code: its terminal-status poll is not a settlement barrier. Agent completion calls
record_agent_completion with settled=false (store/completion_mutations.rs:27-30), publishing
Done and NULL completed_at (:49). Verification happens later in lifecycle/phases.rs:74;
post_run_lifecycle settles Done only after those phases (lifecycle.rs:88). Thus the test can
read the database before verification starts, then drop its temporary home while the worker
is still running. This does not establish an event insertion failure, wrong metadata key, or
a different database. The SQL event predicate is correct; the prerequisite wait is wrong.

The independent audit's contrary claim cites tests/settlement_e2e.rs:111, but that test is
ignored with the reason “task status is terminal before verification settles”. Existing
cmd/wait.rs checks the worker-spec barrier before reading task status; use that contract.
Changing lifecycle status semantics would exceed this correction and is unnecessary here.

Claude final-message selection also needs care: a read-only sample of 100 local JSONL logs
contained 2253 assistant envelopes with absent/null stop_reason and no non-null stop_reason.
Requiring end_turn would silently discard those messages; classifying all assistant envelopes
would still admit interim commentary. The result envelope owns final text and usage. Keep its
Completion event for accounting and derive a separate NoOp from final-text metadata.

### Follow-up changes and validation

The background E2E now uses the harness's gated verifier. While gated it asserts Done,
Pending verification, NULL completed_at, one started event and zero finished events in
that same database. It releases with 0 or 101, waits with `aid wait <id> --timeout 30`, then
retains every original evidence assertion. This corrects the test's observation contract;
it does not change the production lifecycle or weaken the event query.

Help now says “bare --verify uses the project verify command when configured, otherwise
auto-detects”. The pinned assertion checks that wording after normalizing help whitespace.
Codex scans line starts in its completed final message, retaining phase and item restrictions.
Claude no longer scans raw JSON: only successful final result text supplies final_noop_reason
metadata. The watcher derives a NoOp alongside the retained Completion event, preserving
usage accounting. Tool results, tool arguments, interim assistant text, unsuccessful results
and inline quoted markers are negative fixtures. The guide documents the marker boundary.

New/updated Rust coverage (written, not executed in this sandbox):

- background_verify_persists_success_and_failure_evidence: deterministic pre-settlement
  state, then durable evidence after the existing worker-settlement barrier for exits 0/101.
- run_help_describes_verify_shell_semantics: corrected help wording, insensitive to wrapping.
- noop_only_comes_from_completed_agent_delivery: a preface before the final marker now works.
- noop_marker_is_only_classified_in_successful_final_result_text: Claude channel isolation,
  error-envelope exclusion and retained token metadata.
- claude_final_noop_persists_alongside_completion_usage: streams tool/interim/final fixtures
  through the watcher, checks exactly one final NoOp, a Completion, 3 tokens and cost 0.25.

Local validation on the follow-up working tree based on 5345c674:

- A hermetic `python3 - <<'PY'` SQLite reproduction extracted the actual UPDATE SQL from
  src/store/completion_mutations.rs and applied its settled=false parameter. Result: 1 passed,
  0 failed; status=done, completed_at=NULL, finished-event count=0. This proves the premature
  status predicate, not the execution of the new Rust tests. Exact reproduction:

```python
import re, sqlite3
from pathlib import Path
source = Path('src/store/completion_mutations.rs').read_text()
query = re.search(
    r'"(UPDATE tasks SET status = \?1, tokens.*?completed_at IS NULL\)\))"', source, re.S
).group(1)
db = sqlite3.connect(':memory:')
db.execute('CREATE TABLE tasks(id TEXT, status TEXT, tokens INTEGER, duration_ms INTEGER, '
           'completed_at TEXT, observed_model TEXT, attribution_source TEXT, '
           'cost_usd REAL, exit_code INTEGER)')
db.execute("INSERT INTO tasks(id,status) VALUES('fixture','running')")
db.execute('CREATE TABLE events(task_id TEXT,metadata TEXT)')
db.execute(query, ('done', None, 1, '2026-10-21T00:00:00Z', None, None, 0,
                   'fixture', None, False))
assert db.execute("SELECT status,completed_at FROM tasks").fetchone() == ('done', None)
assert db.execute("SELECT count(*) FROM events WHERE task_id='fixture' "
                  "AND json_extract(metadata,'$.verification')='finished'").fetchone()[0] == 0
```

- `rustfmt --edition 2024 --config skip_children=true --emit stdout` on the nine touched Rust
  files parsed successfully. Formatting was applied only to changed/appended functions.
- `git diff --check`: passed. No compilation, cargo, aid build/test or remote E2E ran here.
- The reported 3753-pass/1-fail run belongs to 5345c674; it does not validate this follow-up.
  Remote rerun is required for verify_shell_e2e and the Codex/Claude/watcher marker tests.

No side tasks were dispatched. The earlier note leaving Claude unchanged is superseded by
this explicitly requested correction. The existing lifecycle's provisional Done status remains;
its independent settling barrier is the reason the original test's status poll was incorrect.

Guide quick validation was attempted again with
`python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py default-skills/aid-guide`;
it remains blocked before validation by `ModuleNotFoundError: No module named 'yaml'`.
No environment fallback was installed. Staged credential validation uses
`security-guard --repo . staged` before the follow-up commit.

Required remote commands for the follow-up:

```sh
aid test --test verify_shell_e2e
aid test --bin aid noop_only_comes_from_completed_agent_delivery
aid test --bin aid noop_marker_is_only_classified_in_successful_final_result_text
aid test --bin aid claude_final_noop_persists_alongside_completion_usage
```

KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb verify false passed remote-test noop`.
Matches included “a harness that collected nothing must not grade”, “a failed status poll is not
a failed job”, and “a remote test gate that reports zero binaries is a transport failure”.

## Reproduction and captured facts

The read-only local database records t-8aa27628 as done, verify=auto, verify_status=passed,
effective_dir=/Users/ming/.aid/worktrees/ai-dispatch-5b113037/feat/codex-provider-route,
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
- `python3 /Users/ming/.codex/skills/.system/skill-creator/scripts/quick_validate.py
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

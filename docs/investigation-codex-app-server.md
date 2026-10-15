KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb codex app-server steer`
returned 16 matches, including the warning that writing to a CLI which never reads
its PTY can wedge the steering writer. The follow-up query `codex app-server idle approval`
returned 100 matches, including timeout-recording and configuration-isolation guidance.
The query `codex resume rollout home` returned 64 matches, including the warning that
an isolated HOME does not isolate a CLI with its own config-directory variable.
The query `codex resume mirror` returned 76 matches, including the same isolation warning.
The query `codex mirror drift` returned 178 matches, including mirror-isolation and
regression-evidence guidance. The query `codex mirror config` returned 269 matches,
including config-directory isolation guidance.

# Codex app-server configuration boundary

Status: symlink-only mirror invariant implemented; compilation and Rust suites for this revision
remain pending. The baseline `4ab977eb` passed 3,538 default tests, 3,570 web tests,
and clippy, and completed a writable task with a confirmed live steer. Its retry failed
before producing exec events. The sanitized transcript is preserved at
`tests/fixtures/codex-app-server-steer.jsonl` and drives normalization and fake-server tests.

## Confirmed facts

- `codex --version` reports `codex-cli 0.162.1`.
- `src/agent/codex/capabilities.rs` requires `--ignore-user-config` for both
  read-only and writable launches. The flag prevents loading the user config
  while retaining authentication through `CODEX_HOME`.
- `codex exec --help` documents that flag.
- `codex app-server --ignore-user-config --help` exits 2:
  `error: unexpected argument '--ignore-user-config' found`.
- `codex --ignore-user-config app-server --help` exits 2 with the same diagnostic.
- `codex app-server generate-json-schema --out /private/tmp/aid-codex-schema`
  succeeds. No user-config exclusion parameter was found in the generated schema.
  Thread start/resume accept config overrides, which do not establish exclusion
  of the underlying user configuration.
- The schema includes `thread/resume` with required `threadId`.
- Writable exec launches use `on-request` with `auto_review`; app-server preserves
  those settings. Read-only launches preserve `never` and their native profile.
- `account/rateLimits/updated` carries a sparse `rateLimits` snapshot. Its windows
  include `usedPercent`, `windowDurationMins`, and `resetsAt`; snapshot fields also
  include account/credit and spend-control evidence. Nullable values do not imply
  recovery. No aidbar billing integration has been added.

## Configuration isolation

App-server uses one stable `CODEX_HOME` at `$AID_HOME/codex-home`. Every existing
top-level entry from the real Codex home is symlinked except names beginning with
`config.toml`. Construction is idempotent and concurrent launches accept identical
existing links. Entries pointing at another home fail clearly rather than being replaced.
No entries are copied, and aid does not initialize or edit the real Codex home.
Authentication refresh, session writes, instructions, hooks, rules, and skills still use
the symlink targets. The mirror remains after all task processes and task state are gone.

App-server requires existing `sessions/` and `state_5.sqlite` in the real home, the
rollout state used by Codex 0.162.1. Missing state selects the native exec path with a
diagnostic milestone. On every launch, refresh deletes regular mirror `config.toml*`
files, including trust configuration written by Codex itself. File types are inspected
without following symlinks; config symlinks and directories are preserved and cause fallback.
Symlinks whose real targets have disappeared are removed; missing links are recreated.
The remaining mirror must contain only symlinks into the real home. Any other local state,
including an upgraded `state_6.sqlite`, causes diagnostic exec fallback without deletion.
This replaces the collision-only rule. All precondition failures use the existing exec
fallback without changing native launch settings or creating files in the real home.

The supplied Codex 0.162.1 measurement establishes the boundary: this symlinked
home reports on-request approval, readOnly sandbox, and default effort while still
loading AGENTS.md. The real home reports its configured never/dangerFullAccess/high.
This excludes user TOML without excluding the other home entries that exec loads.

The launch translation retains native `permissions`/`default_permissions` overrides,
approval review, requested model, working directory, and reasoning effort. Resume
uses `thread/resume` only when the existing exec command selected a session.

## Implementation choices

- Select transport at the execution boundary so existing exec command construction
  and fallback tests remain applicable. Direct host Codex commands probe help once.
  Container/sandbox wrapper commands currently retain exec; app-server selection
  inside those execution environments is not implemented.
- Normalize notifications to exec-compatible log envelopes and feed the existing
  streaming accounting, session persistence, and final-delivery guard. Interim usage
  records milestones; only turn/completed produces terminal completion evidence.
  Cumulative usage subtracts the prior-thread baseline, avoiding retry double billing.
- Keep steer requests in the existing pending inbound queue. Persist a receipt event
  keyed by message ID, marking delivered only after a matching successful turn ID.
  Refusals remain undelivered and are not automatically resent during the run.
- Startup responses share a ten-second deadline, bounded further by task idle and hard
  deadlines. Resume excludes replayed turns. Startup failures known to precede a submitted
  turn record their cause and fall back to exec. Explicit thread/resume refusal starts a
  fresh exec session with the retry prompt; other startup failures preserve exec resume.
  A turn/start timeout after submission is ambiguous and is not replayed: the original turn might already have performed work.
- Descendant progress uses the existing activity sampler; genuine idle failures write
  the existing hung-recovery metadata. Steer waits 30 seconds and records timeouts as
  unconfirmed, not refused. Interrupt response and completion waits fit the five-second
  stop grace. Input closes before a bounded final-notification and session-write drain.
- Automatic review remains server-controlled. Client approval requests decline, permission
  requests grant nothing, user-input requests return no answers, and elicitation cancels.
  Legacy approval methods decline as well. Unsupported credential/attestation requests
  fail clearly. Respond remains refused. The installed protocol response schemas are
  authoritative for these payloads; current upstream exec also does not approve client
  requests ([upstream exec request handling](https://github.com/openai/codex/blob/main/codex-rs/exec/src/lib.rs)).
- Tool and other non-message items remain in normalized logs and delivery ordering.
  Exec reasoning summaries retain their prior ignored behavior; app-server reasoning
  has its own envelope. Failure logs and events include capped stderr diagnostics.
- Preserve sparse rate-limit snapshots in event metadata. Exhausted windows feed the
  existing rate-limit marker API only when credits do not permit paid overage; explicit
  spend-control and workspace credit refusals still mark holds. Null fields never clear a hold. No billing contract
  or aidbar integration changes are included.

## Resume persistence

Codex 0.162.1 stores the absolute rollout path in its state database. A thread started
under the temporary home records `tasks/<origin>/codex-home/sessions/...`, even though
`sessions` is a symlink. Deleting that home leaves the rollout intact but makes the
indexed path unavailable. The exec fallback did launch; its resume failed against the
same unavailable path before emitting JSON events. This is independent of sandbox,
HOME isolation, user configuration, and EOF versus forced termination.

The credential-free native probe in `tests/fixtures/codex-resume-home-probe.py` creates
synthetic state with an offline provider. A new app-server resumes through the stable
mirror after the first process has exited and its task state has been removed. The mirror
is refreshed idempotently and never deleted. No lease, origin lookup, origin marker, or
per-task home cleanup is needed. Task active markers retain their existing process-identity
checks and normal teardown removal.

If thread/resume explicitly refuses, exec starts fresh with the retry prompt, environment,
options, and working directory. If an earlier handshake step fails, exec keeps resume;
the stable rollout path remains available. Both PTY and foreground execution share the
same fallback command builder and their tests assert that exec emits events.

## Validation and remaining work

`python3 tests/fixtures/codex-resume-home-probe.py` passed all three native checks on
Codex 0.162.1: first process exit, task-state removal, and subsequent stable-home resume.
No external model endpoint or real Codex home was used. Static checks passed across
26 app-server Rust files with 61 registered test declarations. Tests cover idempotent
and concurrent mirror construction, exclusions, symlink identity, persistent local state,
resume after task-state cleanup, and fallback events in both execution paths. Five new
regressions cover missing sessions through the production command builder, missing state
database, explicit collision fallback, stale-link pruning, and rejected mirror config.
The config rejection tests now cover non-regular config entries; regular mirror config
is removed while real config and linked files are preserved. Two production-builder tests
each exercise both runners: two launches after the fake server writes trust config, and
fallback for unshared mirror-local state. These Rust regressions were written but not run here.

The extra argument in the retry test's `insert_waiting_task` call is removed. No compilation
or Rust suites ran here; there is no tested remote HEAD for this revision. The supplied `719a5bce` baseline passed 3,546 default tests, 3,578 web tests, and clippy.
Its live second launch fell back because Codex had written mirror-local trust config.
That gate did not cover this two-launch behavior. Cross-review, suites, and the live
two-launch check for this revision remain pending; no side tasks were dispatched.

Existing threads indexed through deleted per-task homes are not migrated. Their app-server
resume refusal selects fresh exec. Changing or removing AID_HOME also removes the stable
path guarantee. A real home without session/state files now uses exec; aid does not
initialize real-home files. The earlier guide
validator failure remains `ModuleNotFoundError: No module named 'yaml'`.

Local inspection commands:

```text
codex --version
codex app-server --help
codex exec --help
codex app-server generate-json-schema --out /private/tmp/aid-codex-schema
codex --ignore-user-config app-server --help
codex app-server --ignore-user-config --help
rg -n 'ignore.user.config|configOverrides|configRequirements' /private/tmp/aid-codex-schema
rustfmt --edition 2024 src/runtime/codex_app_server/command.rs src/runtime/codex_app_server/command_tests.rs
python3 /private/tmp/aid-codex-audit-format.py
python3 /private/tmp/aid-codex-audit-static-check.py
python3 /private/tmp/aid-codex-stable-static-check.py
python3 /private/tmp/aid-codex-drift-static-check.py
python3 /private/tmp/aid-codex-invariant-static-check.py
python3 tests/fixtures/codex-resume-home-probe.py
bash /Users/ming/.aid/skills/implementer/scripts/check-file-size.sh src/runtime/codex_app_server
bash /Users/ming/.aid/skills/implementer/scripts/check-headers.sh src/runtime/codex_app_server
bash /Users/ming/.aid/skills/implementer/scripts/check-unwrap.sh src/runtime/codex_app_server
python3 /Users/ming/.codex/skills/.system/skill-creator/scripts/quick_validate.py default-skills/aid-guide
git diff --check
~/.local/bin/security-guard repo "$PWD"
```

The original exclusion-flag probes failed as described; schema generation succeeded.
Formatting applies only to new app-server source files. The operator verification
commands for the final commit are:

```text
aid build check
aid test --bin aid codex_app_server
aid test --bin aid agent::codex
aid test --bin aid cmd::reply
aid test --bin aid cmd::steer
aid test --bin aid cmd::stop
aid test --bin aid cmd::init::official_guide
aid test --test aid_guide_e2e --test init_e2e
```

After these checks, run live steer and stop against Codex 0.162.1, including a resumed
session. Wrapped container/sandbox transport selection remains an explicit limitation.

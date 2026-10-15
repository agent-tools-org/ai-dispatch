KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb codex app-server steer`
returned 16 matches, including the warning that writing to a CLI which never reads
its PTY can wedge the steering writer. The follow-up query `codex app-server idle approval`
returned 100 matches, including timeout-recording and configuration-isolation guidance.
The query `codex resume rollout home` returned 64 matches, including the warning that
an isolated HOME does not isolate a CLI with its own config-directory variable.

# Codex app-server configuration boundary

Status: resume-path recovery implemented; compilation and Rust suites for this revision
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

App-server uses a task-scoped `CODEX_HOME` under the task state directory. Every
existing top-level entry from the real Codex home is symlinked except `config.toml`
and `config.toml.*`. Authentication refresh, session writes, instructions, hooks,
rules, and skills share their existing paths. The owned directory is removed after
app-server teardown, including protocol failures. Existing directories are never
replaced during construction.

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
- Startup responses share the piped exec first-output idle budget. Resume excludes
  replayed turns. Startup failures known to precede a submitted turn record their cause
  and fall back to exec. Explicit thread/resume refusal starts a fresh exec session with
  the retry prompt; other startup failures preserve exec resume and its home lease when
  available. A turn/start timeout after submission is
  ambiguous and is not replayed: the original turn might already have performed work.
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
- Dead-worker state is swept on the next launch. Live worker start identities protect
  in-progress handshakes; owned orphan servers terminate before home removal. Unknown
  live markers from the earlier transport remain preserved because worker custody is absent.
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
synthetic state with an offline provider. It confirms that ID resume fails after deleting
the original mirror and succeeds after recreating that mirror. An additional local probe
of the experimental explicit `path` parameter failed with `cannot resume paginated thread
with stale path`; it does not repair the stored path.

Aid now locates the earliest task carrying the resumed agent session ID and recreates
its isolated home for the duration of the retry. Repeated retries use the same originating
path. Exclusive worker leases prevent concurrent retries from replacing it. An origin
server marker allows stale-worker cleanup to terminate an orphan before removing that
home. The directory and markers are removed after the process ends, as on first launch.
Aid does not edit Codex's shared database or initialize missing files in the real home.
The mirror still excludes user TOML and preserves symlinks and native launch settings.

If thread/resume explicitly refuses, exec starts fresh with the retry prompt, environment,
options, and working directory. If an earlier handshake step fails, exec can still resume
while the restored home lease is held. Failure to acquire or recreate that home records
a diagnostic and selects fresh exec instead of leaving the task without a transport.
Both PTY and foreground execution use the same fallback builder.

## Validation and remaining work

`python3 tests/fixtures/codex-resume-home-probe.py` passed all three native checks on
Codex 0.162.1: missing-origin refusal, restored-origin resume, and home cleanup. No external
model endpoint or real Codex home was used. Static checks passed across 26 app-server Rust
files, including 59 registered test declarations. Nine new Rust regressions cover repeated
resume after cleanup, fresh exec after refusal in both runners, handshake fallback with
exec resume, busy-home fallback, launch preservation, unknown sessions, exclusive leases,
and origin markers. These Rust tests were written but not executed here.

No compilation, Rust suites, or live model task ran for this revision; there is no tested
remote HEAD. The supplied baseline gate is not evidence that this diff compiles. Independent
cross-review and live resume verification remain pending; no side tasks were dispatched.
The generic unwrap script reports test-file unwraps as production; the explicit static
check excludes sibling test files and found no production unwraps.

Recovery requires the originating task record and the same aid state directory. Manual
Codex clients do not recreate aid's temporary home. A real Codex home without existing
session/state files can still create task-local state; real-home initialization is unchanged.
The earlier guide validator failure remains `ModuleNotFoundError: No module named 'yaml'`.

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
python3 /private/tmp/aid-codex-resume-static-check.py
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

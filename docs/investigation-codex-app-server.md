KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb codex app-server steer`
returned 16 matches, including the warning that writing to a CLI which never reads
its PTY can wedge the steering writer.

# Codex app-server configuration boundary

Status: implementation wired; remote compilation, suites, cross-review, and a live
Codex steer remain unverified. The sanitized transcript is preserved at
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
- Bound RPC reads and writes at ten seconds. Task hard/idle deadlines remain bounded.
  Stop requests interrupt the current turn before the existing termination fallback.
- Reject unsupported interactive server requests explicitly. Approval review remains
  server-controlled through the native profile; respond remains refused.
- Preserve sparse rate-limit snapshots in event metadata. Exhausted windows feed the
  existing rate-limit marker API; null fields never clear a hold. No billing contract
  or aidbar integration changes are included.

## Validation and remaining work

No compilation, Rust tests, fake-server execution, browser tests, or live model
calls ran in this session. There is no tested remote HEAD. Twenty-three registered
fixture, launch, home, receipt, and dispatch tests are present for remote execution.
Cross-review remains pending; no side tasks were dispatched.

Local checks: Python AST parsing of the fake-server script passed. Fifteen new
app-server Rust files passed the size, header, line-length, function-length, and
production-unwrap checks. Whitespace and credential scans passed. The supplied
unwrap script reported 50 occurrences from test-only modules; those modules are
registered under cfg(test), and a separate production-only scan found zero. The guide validator could not run:
`ModuleNotFoundError: No module named 'yaml'`. No dependency fallback was attempted.

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
python3 /private/tmp/aid-codex-static-check.py
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

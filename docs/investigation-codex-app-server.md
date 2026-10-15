KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb codex app-server steer`
returned 16 matches, including the warning that writing to a CLI which never reads
its PTY can wedge the steering writer.

# Codex app-server configuration boundary

Status: blocked; no runtime behavior has changed. The launch translator under
`src/runtime/codex_app_server/command.rs` is an unwired draft, not a working adapter.
Its sibling tests are also unwired and have not run. The supplied sanitized
transcript is preserved at `tests/fixtures/codex-app-server-steer.jsonl`.

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
- Existing writable exec launches use `on-request` with `auto_review`.
  The requested app-server behavior instead specifies `never`. That discrepancy
  needs to remain explicit when implementation resumes.
- `account/rateLimits/updated` carries a sparse `rateLimits` snapshot. Its windows
  include `usedPercent`, `windowDurationMins`, and `resetsAt`; snapshot fields also
  include account/credit and spend-control evidence. Nullable values do not imply
  recovery. No aidbar billing integration has been added.

## Unresolved boundary

App-server availability alone is insufficient to preserve the current launch
contract. Dropping `--ignore-user-config` would load configuration that exec
explicitly excludes. Copying credentials into another home or replacing config
files would introduce a different isolation mechanism. Neither is implemented.
The draft rejects this flag rather than silently losing its protection.

Possible next directions:

1. Add equivalent config-exclusion support to the Codex app-server binary.
2. Establish a documented protocol equivalent that excludes the user config.
3. Revise the configuration-isolation requirement explicitly before designing
   another launch mechanism.

## Validation and remaining work

No compilation, Rust tests, fake-server dispatch tests, or live model calls ran.
Remote compilation is unavailable in this session. There is no tested remote HEAD.
The draft has three regression cases, but they are not registered in the crate.

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
```

The two exclusion-flag probes fail as described; schema generation and formatting
succeed. All transport wiring, event mapping, confirmed steer receipts, interrupt
handling, quota evidence integration, guide changes, fake-server coverage, and
cross-review remain outstanding. No side tasks were dispatched.

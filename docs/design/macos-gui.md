# macOS GUI — design

Status: draft for review, 2026-10-24. No implementation starts before this design is accepted.

KB consulted: `a-stub-that-agrees-with-the-code-proves-nothing` (contract fixtures must come from
the real serializer), `the-named-wire-authority-can-be-the-stale-sibling` (`client-api.md` has
drifted from the server), `an-app-store-connect-api-key-cannot-mint-a-developer-id-certificate`
(Developer ID signing has a manual portal step).

Inputs: the existing AID Command client (`client/`, spec `client/DESIGN.md`), the
[client API contract](client-api.md), and an inventory of both against the server at `7543e6c3`.

## 1. Starting point

AID Command is a SwiftUI app for macOS 14+ and iPadOS 17+, about 7,000 lines. Its macOS scheme
builds and its tests pass with Xcode 27.1 (42 tests, 1 skipped). Every endpoint in the contract
is registered in `src/ui/web/`, and Swift decodes the field names and types the server sends.
The gaps are semantic:

| Area | Finding |
| --- | --- |
| Outcome | The client maps outcomes `timeout`, `infra` and `no_result`, which the server never emits. `unverified`, `stopped` and `skipped` fall through to BROKEN; a failed task shows VFAIL and a failed verification shows BROKEN. `verify_status` (the reason for `unverified`) is not decoded |
| Delivery | `delivery_assessment` (`hollow_output`, `missing_final_delivery`) is ignored, so a task the server counts as failed can show COMPLETE |
| Status | `waiting`, `pending`, `awaiting_input` and `stalled` all show RUNNING; unknown future statuses also become RUNNING |
| Invented values | Progress percentages, rank and XP, radar positions, payload priority, "reactor load", hangar bays and unit levels are computed in the client with no server source. A null fleet memory shows `0 MB` |
| Stale data | An empty live snapshot keeps the previous sectors on screen, including the initial demo data |
| Actions | Stop, retry, steer, merge, diff and report export are wired. Respond, accept and reject exist on the server with no client action. Retry discards the new task ID |
| SSE | The client refetches `/api/fleet` on every notification; the server polls every 2 s and emits task updates only on status changes, so token, milestone, outcome and error changes alone produce none |
| Errors | Refusals from shared retry/steer/respond/accept/reject handlers return 500 instead of 409 |
| Distribution | Release binaries are built without the `web` feature, so a released `aid` cannot serve the app |

## 2. Users and jobs

The user dispatches work with `aid` (directly or through an orchestrating agent), runs several
tasks at once across projects, and wants to know without reading terminals:

1. **What needs me now** — tasks awaiting input, stalled, failed, failed verification, or done
   and waiting for acceptance.
2. **What a task did** — prompt, events, output, report, diff, verification result, cost, model.
3. **Act on it** — steer, respond, stop, retry with feedback, accept or reject, merge.
4. **Capacity** — which agents are busy or held by quota, and today's spend.
5. **Start a task** — later phase (section 7).

Not in scope: aid configuration, agent/provider/credential management, retention, teams,
knowledge and memory. Those stay in the CLI.

## 3. Display rules

These hold whatever the visual style:

- **Unknown stays unknown.** A null is `—`. No progress percentage, no derived load, no score.
- **One derivation, in Rust.** The server sends display facts it already computes (status,
  outcome, verification tag and reason, delivery assessment, attention reason). Swift renders
  them and does not re-derive them. A value Swift does not recognise renders verbatim, never
  as a guessed state.
- **Lifecycle and result are separate axes.** A row shows status and, beside it, the
  verification or delivery tag. A done task with failed verification shows both.
- **Live and fixture data never mix.** Fixture data exists only for previews and tests; the
  running app has no demo mode. A disconnected window says so and clears or greys stale rows.
- **aid's own terms.** Task, project, group, agent, model, worktree, cost, as in the CLI.

## 4. Data path

| Option | For | Against |
| --- | --- | --- |
| **A. `aid web` HTTP + SSE (chosen)** | Exists; actions reuse the CLI handlers; auth exists; the same binary owns all semantics | Release binaries lack `web`; something must run the server |
| B. Read-only SQLite | No server; reads are lock-free since v11.7.0 | Swift duplicates schema knowledge and outcome derivation (the drift above); actions still need the CLI |
| C. `aid … --json` subprocesses | No server | A process per refresh, no streaming, and most `--json` outputs are not a stable contract |

**Managed local server.** On launch the app looks for a running server it started earlier;
otherwise it starts `aid web --host 127.0.0.1 --port 0` as a child process and stops it on
quit. The `aid` path is resolved from Settings, then `PATH` (`~/.cargo/bin`, `/opt/homebrew/bin`).
A remote server (URL + token in the Keychain) remains a Settings option for a LAN machine.

**Loopback is still authenticated.** A browser page can send requests to `127.0.0.1`, so the
managed server always requires a token: the app generates one per launch and passes it through
the environment (`AID_WEB_TOKEN`), not argv, which other users can read with `ps`. The server
also rejects requests whose `Host` is not the bound address (DNS rebinding).

**Port 0.** `aid web` binds an ephemeral port and prints one machine-readable ready line
(`{"listening":"http://127.0.0.1:<port>"}`) that the app reads from stdout.

**Release builds include `web`.** The extra crates are axum, tower-http, rust-embed and
futures; tokio is already a dependency. The size difference is to be measured on the build box
before the change; CI already builds and tests both feature sets. This settles the first M2
item in the roadmap.

**Version check.** `/api/fleet` returns `server.version` and a new integer `api_version`. The
app states the range it supports and shows "aid X is too old/new for this app" instead of
decoding partially.

## 5. Server changes

All additive to `/api/`, with no parallel API. Each lands with a Rust test.

| # | Change |
| --- | --- |
| S1 | Task responses carry `verification_tag` (the server's existing tag helper), `verify_status`, `delivery_assessment` and `attention` (`awaiting_input`, `stalled`, `failed`, `verify_failed`, `needs_acceptance`, or null), plus the principal acceptance state |
| S2 | SSE `task_update` fires when any displayed fact changes (status, outcome, verify status, tokens, cost, milestone, latest error), and carries the task row so the client patches in place; `agent_update` fires on any quota field change |
| S3 | Refusals from shared action handlers return 409 with the handler's message; only unexpected errors are 500 |
| S4 | `aid web --port 0` ready line, `AID_WEB_TOKEN`, `Host` check, `api_version` |
| S5 | A Rust test writes fixture JSON from the real serializers (`fleet`, task, events, SSE payloads) into `client/AIDCommandTests/Fixtures/`; Swift tests decode exactly those files, and a Rust test fails if the checked-in fixtures are stale |
| S6 | `client-api.md` is regenerated from S1–S5 and its contradiction on non-loopback tokens is resolved (generate and persist a token, as the code does) |

## 6. Screens (native direction)

The visual direction is pending review (native macOS or the existing fleet console). This
section describes the native option; the data rules and server changes above apply to either.

One window, `NavigationSplitView` with three columns:

- **Sidebar.** Smart lists: Needs you (badge), Running, Failed, Ready to accept, All history.
  Projects and groups with counts. Agents with busy/idle and quota state (`held 41m` with the
  recovery time on hover). Today's spend.
- **Task table.** `Table` with sortable columns: status, task (title + id + project),
  agent · model (observed model when known, marked when it differs from the requested one),
  verification tag, elapsed, tokens, cost. Grouped by attention in the default list.
  Search covers title, id and prompt.
- **Inspector.** Status line, title and id; attention banner (with the awaiting question and
  a Reply field for `awaiting_input`); facts (agent, project, profile, worktree and branch,
  verify command and result); tabs Activity (full event list), Output, Report, Diff, Prompt.
  Long outputs load on demand.

Actions by state, from the toolbar, the inspector and the Task menu:

| State | Actions |
| --- | --- |
| running | Steer…, Stop |
| awaiting input | Reply…, Stop |
| stalled | Steer…, Stop |
| failed, stopped | Retry… (feedback sheet; selects the new task) |
| done, not accepted | Accept, Reject…, Merge…, Diff |

Merge, Reject and Retry confirm first; Stop does not. A 409 shows the server's message in
place; nothing is retried or faked.

Also: a menu bar item (count of running tasks and a badge for Needs you, with a list that
opens the task), user notifications on transitions into Needs you and on completion
(configurable per kind), standard menus and shortcuts (`⌘F` search, `⌘R` refresh, `⌘.` stop,
`⌘⏎` reply), system light and dark appearance.

## 7. Phases

| Phase | Scope | Acceptance |
| --- | --- | --- |
| P0 | S1–S6; `web` in release builds | Default and web suites, clippy; fixture round trip passes in Rust and Swift |
| P1 | Read-only app: managed server, sidebar, table, inspector, menu bar item, notifications | UI test on this Mac against a `aid web` serving a fixture `AID_HOME`: each attention kind appears in Needs you; a status change arrives over SSE without a refetch; a disconnected server is shown as such |
| P2 | Actions in section 6 | UI test per action, including one 409 shown in place and retry selecting the new task |
| P3 | New task from the GUI | Needs its own design: a dispatch endpoint exposes agent launch over HTTP and requires a security review first |

## 8. Kept and dropped from AID Command

Kept: `SSEClient`, `KeychainTokenStore`, connection settings, the live source and decoder
(reworked for S1–S2), the xcodegen project, the test targets.
Dropped with the native direction: rank and XP, radar, payload rarity, reactor load, hangar
bays, unit catalog and levels, the demo tick engine at runtime, the theme system, and the
game vocabulary. The iPad target is dropped for the first release; SwiftUI keeps the
option to restore it later.

## 9. Distribution

- Development builds are unsigned (the current project disables signing).
- Public builds need Developer ID signing and notarization. Creating the Developer ID
  certificate is a manual step in the Apple developer portal by the account holder; the rest
  can be scripted.
- The app ships as a notarized `.zip` or `.dmg` attached to the GitHub release, built on a
  macOS runner; the app version follows the aid version.

## 10. Open questions for review

1. Visual direction: native macOS (section 6) or the existing fleet console.
2. Include `web` in release binaries (pending the size measurement).
3. Drop the iPad target for the first release.
4. Keep remote (LAN) server access in P1, or loopback only.
5. Signing: which Apple developer account signs and notarizes.

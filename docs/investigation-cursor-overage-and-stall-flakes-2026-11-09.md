KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb overage billing quota window degraded`.
No entry covered spend counters read as billing state; the lesson was added as
`ai-coding/a-cycle-spend-counter-is-not-the-billing-state.md`.

## Cursor shown OVERAGE and DEGRADED 100% (fixed in v11.13.1)

CONFIRMED from the raw `usage-summary` response: plan used 665 of 2000, on-demand enabled with
100 of 100 used. The next request was billed in plan. Two readings were wrong:

- the usage probe labelled the route `overage` whenever on-demand spend this cycle was above
  zero (fixed in the probe: state now follows plan headroom, then on-demand headroom);
- `max_used` included the `on-demand` window, so a capped spend budget showed as 100% capacity
  and `snapshot_overrides` refused to release a hold.

Fix: `src/routing/quota/live.rs` drops windows in the `on-demand` group from capacity evidence.
Spend state still reaches routing through billing evidence.

AMBIGUOUS: the response's `totalPercentUsed` (1.34) disagrees with used/limit (33%). Its meaning
is unconfirmed.

## Release-gate flakes found on the way

1. `read_only`: on a box whose checkout sits under a mount point, `git rev-parse` reports
   `fatal: not a git repository (or any parent up to mount point /)`. The prefix match now covers
   both forms; `fatal: not a git repository: <gitdir>` (missing gitdir) stays an error.
2. Watcher stalls: failures reproduced only under whole-process SIGSTOP stalls (3 s stall:
   10/10 failing before, 0/10 after). Misbilled and cost-ceiling stops use
   `terminate_child_group`; the idle watchdog reads ready output or EOF once before declaring a
   hang.
3. Reaper: a concurrent `aid wait`/`show`/`board` killed a worker that was settling its own hang
   verdict, so HUNG never became a retry (`tests/caller_owned_recovery.rs` failed 10/10 before,
   passed 10/10 after). `cleanup_wedged_live_worker` skips the reap only while the newest
   `hung_recovery_eligible` event is younger than `HUNG_SETTLE_GRACE_SECS` (30 s).
4. The codex preflight test stub accepts `--ignore-user-config` (24 tests failed on codex 0.166.0).

## Open

- `src/cmd/run/process_watch.rs` error and hard-timeout paths still block on a 3 s kill grace.
- The piped streaming watcher records its verdict after its 3 s grace; the reaper's first-token
  clock starts at task creation.
- `post_hung.rs`: a leftover Codex app-server hang verdict makes later exec-run failures retry
  as hangs.
- Seen once, not reproduced: `runtime::claude_input::exit_tests::broken_pipe_while_sending_steer_stays_unconfirmed`,
  `bounded_probe_e2e` hung agy help.
- Inside the Codex seatbelt sandbox `tailscale status` fails, so codex executors cannot reach
  build boxes.

1. **FAIL.** `maybe_handle_idle` refreshes `last_progress_time` every monitor tick when log mtime is within `warn_after` (`src/pty_watch.rs:330`, `442-448`). A steadily-writing buffered agent therefore keeps resetting the same clock checked by `idle_hang_elapsed` (`src/pty_watch.rs:424-430`), so the idle reaper never fires. It is only bounded by the separate background max-duration kill—default approximately 60 minutes—not the idle timeout.

2. **PASS, with the coupling caveat.** The ladder directly uses `warn_after`; the reaper directly uses `idle`. Streaming agents skip refresh and buffered-log checks (`src/pty_watch.rs:427-428`, `443-445`). However, the ladder’s refresh mutates the reaper’s clock, causing the failure above.

3. **FAIL.** The removed marker has no remaining writer, reader, accessor, test, or exact reference. The remaining source documentation honestly says sandbox/container runs lack buffered liveness (`src/agent/mod.rs:115-117`). But stale claims remain in `CHANGELOG.md:7` and `docs/investigation-quota-routing-2026-08-08.md:20-22`, saying idle buffered liveness is still unsupported.

Overall: **FIX**

What I missed: the new tests cover isolated single calls, but none simulate repeated monitor ticks with steady log growth; such a regression test would have exposed the clock-reset defect.
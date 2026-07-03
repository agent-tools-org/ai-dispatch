# Investigation: droid tasks hang/fail under aid (OSC escape incompatibility)

**Date**: 2026-07-03 · **Status**: root cause confirmed · **Fix**: proposed, not yet applied

## Problem

All droid tasks dispatched via aid fail since 2026-06-29. Two failure shapes:
- Trivial prompts ("reply pong") fail within seconds (t-43da6329, t-6e19cbeb, 2026-06-29).
- Longer read-only tasks die with `Agent hung: no output for 300 seconds` (t-ff094361, 2026-07-03) while the event log contains only terminal escape junk (`]0;⛬ …` and repeated `]9;4;3;`).

## Evidence

1. **droid 0.159.1 installed 2026-06-29 16:39** (`~/.local/bin/droid`) — failures start the same day.
2. **Piped stdout (no TTY): clean.** `droid exec --output-format stream-json "…pong"` via `subprocess` pipe returns pure line-JSON (`system/init → message → completion`, 3.4 s).
3. **PTY stdout: JSON lines get OSC prefixes.** Same command under `pty.fork()` returns:
   ```
   \x1b]0;⛬ <prompt>\x07{"type":"system",…}
   \x1b]9;4;0;\x07\x1b]9;4;3;\x07{"type":"message","role":"user",…}
   {"type":"message","role":"assistant","text":"pong",…}
   \x1b]9;4;0;\x07{"type":"completion",…}
   ```
   `\x1b]0;…\x07` = window title (contains the prompt), `\x1b]9;4;n\x07` = terminal progress protocol. droid ≥0.159 emits these when stdout is a TTY, glued to the front of JSON lines.
4. **aid runs every agent under a PTY** (`background.rs:174 → pty_runner.rs → pty_bridge.rs`, 24×80).
5. **Parser breaks on the prefix.** `DroidAgent::parse_event` (`src/agent/droid.rs`) does `serde_json::from_str(line)` on the raw line — any OSC-prefixed line fails and yields no event.
6. **Idle watchdog counts parsed events only.** `mark_progress()` fires solely on parsed progress events (`pty_watch.rs:114-118`); raw bytes don't reset the timer. With most/all lines OSC-prefixed, `last_progress_time.elapsed() > idle` triggers the hung kill (`pty_watch.rs:401`).
7. The existing `strip_ansi` (`pty_watch.rs:567`) strips only CSI (`\x1b[`) sequences, not OSC (`\x1b]…\x07`), and is used only for awaiting-prompt extraction — never on the event stream.

## Root cause

droid 0.159.x added terminal-integration output (window title + OSC 9;4 progress) in `exec` mode when attached to a TTY. aid's PTY-based capture therefore receives stream-json lines with OSC prefixes, its per-line JSON parser drops them, and the idle watchdog kills the task. A completion event that fails to parse also marks fast tasks as failed despite a correct answer.

## Fix proposals

1. **(Preferred) Strip OSC + CSI sequences at the stream choke point** — in `watcher/stream.rs` before `agent.parse_event(...)` (or extend `strip_ansi` to handle OSC `\x1b]…(\x07|\x1b\\)` and apply it there). Generic: protects every agent adapter from terminal-integration escapes. Low risk; JSON payloads never legitimately contain raw `\x1b`.
2. (Not recommended) Reset the idle timer on raw bytes — would keep genuinely hung agents alive on spinner pulses.
3. (Upstream) Report to Factory.ai: `exec --output-format stream-json` should stay machine-clean even on a TTY. Worth filing but not a fix we can wait for.

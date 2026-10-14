KB consulted: `steer acknowledgement` returned eight matches, including
`ai-coding/a-cli-that-never-reads-its-pty-wedges-the-steering-writer.md`.

# Steering transport and acknowledgement

The Codex adapter launches `codex exec --json --skip-git-repo-check`, or a
separate `codex exec resume` invocation at task launch. Exec is non-interactive.
Writing plain text to its PTY does not submit another model-facing user turn.
The interactive-input capability previously returned true despite this mode.

The steering path queues an inbound database message and writes a signal file.
The PTY monitor consumes that file and writes the payload plus a newline to the
terminal. A successful write proves terminal acceptance, not model consumption.
There is no steer-triggered resume, hook queue, or next-turn input protocol.

Two captured executions recorded steering and acknowledgement events despite
absent model delivery. Their first payloads appeared only as partial terminal
echoes, cut at 1,022 characters and followed by BEL bytes. The later correction
payloads were absent from the complete raw logs. All four payload prefixes were
absent from a complete 3,335-line Codex rollout and the assistant outputs.
Later retry prompts were separate user turns. The truncation and BEL bytes are
consistent with line-discipline rejection; exact disposition of every byte was
not traced.

The output monitor previously acknowledged the latest delivered inbound message
on any non-empty line. Echo detection excluded echoes from parsing and progress,
but did not exclude them from acknowledgement. Unrelated output could therefore
set `acked_at` and emit `Acked reply`. Selecting the latest delivered message also
allowed multiple messages to be acknowledged in reverse order. No message ID or
protocol receipt connected the output line to consumption of the input.

Codex input must be refused before message insertion or signal creation.
`aid retry <id> -f "<feedback>"` starts another attempt and stops a live worker
first; explicitly stopping and then retrying is also available. Resume support
applies at launch and does not provide mid-run steering.

For PTY-capable adapters, successful input forwarding means **written to PTY;
consumption unconfirmed**. Arbitrary subsequent output cannot acknowledge it.
There is no real receipt writer for `acked_at`; its active storage and readers
must be removed rather than presenting output activity as a receipt.

The remaining built-in true capability declarations are Gemini, Qwen, Copilot,
CommandCode, Cursor, Droid, Oz, OpenCode, Kilo, and MimoCode. Custom adapters are
configurable and default to true. Claude, Grok, and Antigravity already refuse
interactive input. These declarations establish permission to attempt a PTY
write, not evidence of genuine live consumption. The remaining adapters' CLI
input loops and real receipts were not verified, so their flags are unchanged.

The original investigation was read-only and ran no builds or tests. Its evidence
establishes the Codex failure and false acknowledgement; it does not establish
which particular output line triggered every historical acknowledgement.

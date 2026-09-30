# Classify: typed questions about an artifact

`aid classify` (and the MCP `classify` tool) asks TypeSafe Jev, a System One
model, small typed questions about a text or JSON state and prints a small JSON
answer. Use it when the dispatcher needs one fact from a large artifact (an audit
report, a delivery summary, a log) without reading the whole artifact into its
own context.

Question types:

- `noul`: yes/no; the answer is the probability of yes (0..1).
- `choice`: one of 2-255 declared options, with `confidence` and `probabilities`.
- `score`: a probability-weighted value across 2-10 ordered levels (index 0 first).

## Key setup

The key lives only in the macOS login keychain. Run once, in a real terminal (the
prompt needs a TTY):

```bash
security add-generic-password -a "$USER" -s typesafe-api-key -U -w
```

AID reads it at call time with a 3 s bound. There is no environment-variable
fallback: an exported key would be inherited by every dispatched agent. Without a
key, `aid classify` exits 2 and prints this command.

## What is sent where

One HTTPS `POST https://api.typesafe.ai/v1/systemone` carries `state`, `model`
(default `jev-1.13.0`), and the questions map. The key and the request body reach
`curl` on stdin, never in argv or the environment. Before sending, AID refuses
(exit 5) state over 100,000 characters (it never truncates) and state with a PEM
block or a token prefix at a word start (`sk-`, `ghp_`, `github_pat_`, `xox`,
`AKIA`, `AIza`, `ts_`); `--allow-secret-like` sends it anyway. The MCP tool
always screens. Single-item mode retries a 429 or 529 once after a short backoff.

## Command

```text
aid classify [--state <file>|-] [--state-json]
             [--noul ID=QUESTION]...
             [--choice ID=QUESTION --options A,B,C]...
             [--score ID=QUESTION --levels L0,L1,...]...
             [--questions <file.json>] [--model jev-1.13.0] [--timeout SECS]
             [--allow-secret-like]
```

State defaults to stdin. Each `--choice` pairs with the next `--options`, each
`--score` with the next `--levels`. `--questions` takes the native TypeSafe map
(`id -> {type, instructions, criteria}`) and merges with the flags; an id used
twice is an error.

Output (stdout, always JSON):

```json
{"model":"jev-1.13.0","answers":{"verdict":{"type":"choice","choice":"SHIP","confidence":0.9,"probabilities":{"SHIP":0.93,"FIX":0.05,"BLOCK":0.02}}},"usage":{"input_tokens":312},"latency_ms":640}
```

AID validates the response: `model` must be `jev-<version>`, every answer must
match its question's type, choices and probability keys must be declared
options or levels, and numbers must be finite and in range.

| Exit | Meaning |
|---|---|
| 0 | ok |
| 2 | no key (the setup command is printed) |
| 3 | API or network error (HTTP status and API error type only) |
| 4 | invalid questions or flags |
| 5 | state refused (secret-like or too large) |
| 6 | batch completed with one or more failed items |

Errors are one line on stderr and never include the key.

## Batch mode

Run the same questions over a JSONL file:

```bash
aid classify --batch items.jsonl --out results.jsonl --jobs 4 --resume \
  --choice verdict='What final verdict does this audit give?' --options SHIP,FIX,BLOCK
```

Each input line must be an object with a unique, non-empty string `id` and a
`state`. String states are text; other JSON values use the same screening as
`--state-json` (only non-empty objects or arrays pass). All lines and ids are
validated before any request is sent. Missing/duplicate ids, malformed JSONL,
invalid questions, model, timeout, or output paths exit 4. Do not use the same
file for input and output. `--batch` requires `--out` and conflicts with `--state`
and `--state-json`. `--out`, `--jobs`, and `--resume` require `--batch`.

```jsonl
{"id":"audit-1","state":"Verdict: SHIP. Tests passed."}
{"id":"audit-2","state":{"verdict":"FIX","reason":"missing tests"}}
```

The existing question flags, `--questions`, `--model`, `--timeout`, and
`--allow-secret-like` apply to every item. Each state is screened independently;
one refusal or API failure does not stop siblings. The key stays keychain-only
and is read once for a non-empty pending batch; missing key exits 2. An empty or
fully resumed batch needs no key. No batch MCP tool is provided.

`--jobs` defaults to 4 and accepts 1-16. Workers use a shared cooldown on every
HTTP 429/529, doubling from 1 second to a 32-second cap. After throttling, all
new attempts are paced by that delay for the rest of the batch; requests already
in flight can finish. Each item gets at most five attempts. Other failures are
recorded immediately. Single-item retry behavior is unchanged.

Results append to `--out` and flush as each item completes, in completion order;
stdout stays empty. Successful rows contain `id`, `ok:true`, `answers`, `model`,
`usage`, and `latency_ms` (including batch pacing/retries). Failed rows contain
`id`, `ok:false`, `error_kind` (`no_key`, `api`, `invalid`, or `refused`), and a
sanitized `error`. State and credentials are never copied into result rows.

`--resume` skips input ids with any previous `ok:true` row in `--out`; failed ids
are retried. Without it, every input item runs and new rows still append. A
missing output file starts fresh. Malformed resume rows, including a truncated
final row from an interrupted write, exit 4; repair that row before resuming.
Use an output file associated with the same input ids and questions: resume
matches ids only. Only one batch process should write a given output file.

The final stderr summary prints `total / ok / failed / skipped` as named counts
and a count for every option of each choice question, including zero counts.
`ok` and option counts cover successes from this invocation; skipped rows are
not counted again. Exit 0 means no pending items failed (including all-skipped
batches); exit 6 means some items failed. An output write failure exits 4, stops
new work as workers finish, and preserves already flushed rows.

## Examples

Audit-verdict extraction:

```bash
aid show t-1234 --output --full | aid classify \
  --choice verdict='What final verdict does this audit give?' --options SHIP,FIX,BLOCK \
  --noul evidence='Does the audit cite test output or command output as evidence?'
```

Does this delivery show test output:

```bash
aid show t-1234 --output --full | aid classify \
  --noul tests_ran='Does this text contain output from an actual test run (test names, pass/fail counts)?' \
  --score coverage='How much of the change do the shown tests exercise?' --levels none,partial,most,all
```

## Hints, never a verdict

An answer over agent output or logs is a hint about what the text says, not
evidence that it is true. An agent can write "12 passed" without running a test.
Use a classify answer to decide where to look next; the verdict still comes from
running the tests, reading the captured log, or an independent audit.

## Known weak spots

Jev is documented as weak at numbers, dates, and counting. Do not ask it to
compare quantities, compute or order dates, or count occurrences; extract those
with a parser or read them directly.

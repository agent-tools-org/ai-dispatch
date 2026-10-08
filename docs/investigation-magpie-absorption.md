# Investigation: mechanisms in magpie applicable to aid

KB consulted: `kb quota rate limit cascade failover`, `kb env isolation HOME leak`,
`kb token cost cache pricing`. Relevant hits: `ai-coding/failover-granularity-must-match-metering.md`
(applies to items 1, 7), `ai-coding/passthrough-symlink-sandbox-records-ephemeral-paths.md` (item 9).

Source studied: github.com/yetone/magpie at `e4c292f` (2026-10-08), a Go local LLM gateway that
wires 35+ coding-agent CLIs to many vendors and fails over between accounts. aid is not a proxy:
it sees CLI stdout/stderr, not HTTP, so only mechanisms that survive that difference are listed.
Magpie paths are relative to its `internal/`; aid paths to `src/`. "Verified" means the aid claim
was re-read in the code; otherwise it is a research finding still to confirm.

## Summary

Status is as of 10.51.0.

| # | Area | Defect or gap found in aid | Status |
|---|------|----------------------------|--------|
| 1 | Quota-aware ranking | Headroom penalty read all windows, ignored model scope and reset time | Scoped to relevant windows; expired and soon-resetting short windows ignored. Reset-time ranking open |
| 2 | Failure taxonomy | Context overflow retried on the same model with stderr prepended | Open |
| 3 | Vendor error signatures | A Claude quota failure was not classified as quota from CLI output | Claude refusals recognised from CLI envelopes. Other vendor patterns open |
| 4 | Quota reading | Snapshot lacks credits/held/span; Droid missing from refresh list | Droid added. Snapshot fields open |
| 5 | CLI on another vendor | Provider always the CLI default; no endpoint wiring for claude/codex | Open |
| 6 | Model catalog | Exact-match rating lookup; static price beats live feed; Codex probe keeps hidden models | Open |
| 7 | Multiple accounts per CLI | No account dimension in route, markers or homes | Open |
| 8 | Token/cost accounting | Blended 70/30 estimate priced cache reads as input; OpenCode kept the last step only; alphabetical model pick | Component pricing, step summing for OpenCode-format parsers, dominant model pick. Resume/subagent coverage open |
| 9 | Per-CLI env inventory | No single list of per-CLI config/home variables | Inherited redirect variables removed at launch. Shared inventory for sandbox/container lists open |
| 10 | Early failover / exhaustion | Fast non-quota failure uses `--retry` before cascade; all-held error names one route | Open |

## 1. Quota-aware route ranking

- Magpie (`gateway/routing.go` `weighRouted` ~664-906, `provider/routing.go` `Allowance`):
  tiers by share used (fine <90, low 90–98, spent ≥98); within "fine", the account whose largest
  window renews soonest first (reset truncated to the hour, unknown last); `pace` mode ranks by
  `(100-used)/max(hours_to_reset,1)` on the tightest window of span ≥24h. Only windows whose
  model scope covers the requested model count. Stable sort keeps config order on ties.
- aid: `agent/selection_quota.rs:13` `headroom_penalty` takes the tightest of *all* probe windows
  (50/80/95 → 0/−1/−3/−6). `resets_at` is unused, although `docs/design/quota-awareness.md:123`
  says advise ranks time-to-reset. `credential_pool.rs::get_credential` has no production caller.
- Defects: a full agy gemini pool penalises a claude-model request; a codex 5h window at 85%
  resetting in 20 minutes costs −3.
- Applicable mechanism: scope windows with the existing `relevant_windows`; tier per budget window; an additive
  pace term (bounded, so quota never overrides capability across different CLIs).

## 2. Failure taxonomy and hold policy

- Magpie (`gateway/routing.go` `failure()` ~191-212, `restAfterMarked` ~362-449): classes
  credit (30 min), quota (vendor reset, else window renewal, else Retry-After, else 15 min, cap
  8 d), rate (1 min doubling within 30 min, cap 30 min), verify, proxy, other (1 min doubling, cap
  10 min). Request-content failures (overflow, safety refusal, rejected system prompt, bad shape)
  rest nobody; overflow keeps only candidates with enough context (`withRoom`). Success clears
  account and account+model rests.
- aid: quota-only classes (`rate_limit_signatures.rs:33-50`); any generic 429/402/"rate limit"
  becomes a 300 s advisory `Transient` hold. No rate, credit, overflow or refusal class.
- Defects: "prompt is too long" goes to `prepare_retry` on the same model with the stderr tail
  prepended, so the retry is longer and fails again; repeated bare 429s never escalate; a 429 early
  in stderr followed by an unrelated failure still writes a hold.
- Applicable mechanism: `failure_class.rs` with `{Quota, Rate, Credit, Auth, Overflow, Refused, Other}` over
  text `quota_channel` already attributes; overflow/refused write no marker and cascade to a
  different model; rate backoff gated from the second hit. Overflow needles need real captures.

## 3. Vendor and CLI error signatures

- aid has no `AgentKind::Claude` row in `rate_limit_signatures.rs`, and no source or test mentions
  "hit your limit" or "usage limit reached". Claude stream errors reach only the generic check
  (`rate_limit.rs` `generic_quota_signal`: "rate limit", 429, 402, "too many requests"), which
  "You've hit your limit · resets 5pm (Asia/Shanghai)" does not match. A Claude hold can still come
  from the live quota snapshot (`live_quota` → `route_availability`), but only while that snapshot
  is fresh; the task-end failure itself is not recognised as quota, so it takes the same-agent retry
  path instead of quota continuation. The generic check excludes
  snake_case `rate_limit` on the premise that no provider emits it; magpie's Claude stream fixture
  (`gateway/claude_made_first_test.go:71-74`) carries `"error":"rate_limit"` and a
  `rate_limit_event` with `status:"rejected"` and `resetsAt`. That fixture is synthetic, so a real
  capture from the CLI comes first.
- Further patterns in magpie (`gateway/routing.go:106-133`, `gateway.go:~4700`, fixtures in
  `ratewords_test.go`, `routing_test.go:184-190`):
  - Zhipu `{"code":"1113","message":"余额不足或无可用资源包,请充值。"}` on 429 = out of credit;
    `insufficient_quota`, `欠费`, `请充值`, `Your credit balance is too low`.
  - Codex `{"type":"usage_limit_reached","resets_at":<unix>,"resets_in_seconds":N}`; bare
    `try again at 9:34 PM` with no date.
  - Claude `limit reached|<10-digit unix>`; OAuth `session expired and could not be refreshed`,
    `token revoked` (auth, not quota).
  - Relay quota disguised as rate limit: `限额已用完`, `额度`, `套餐`; true rate wording:
    `per minute`, `TPM`, `RPM`, `频率`, `太频繁`.
  - Context overflow: Kimi `exceeded model token limit`, MiniMax `context window exceeds limit`,
    z.ai `Prompt too long`, DashScope `range of input length should be`.
  - Model not served: `model_not_supported`, `模型…不存在|不支持|无权|未开通`; Gemini
    `VALIDATION_REQUIRED` needs a person.
- Applicable mechanism: a Claude row with reset parsing; an agent-independent credit row so Chinese balance
  wording outranks the generic 429; Codex JSON reset fields; auth rows in `auth_marker.rs`.

## 4. Reading subscription quota

- Probes live in aidbar by design (`docs/design/quota-awareness.md:134,147-149`); aid parses its
  snapshots (`live_quota.rs`). Magpie sources per subscription: Codex `wham/usage` (with the
  `chatgpt-account-id` header) and `wham/rate-limit-reset-credits`; Claude via `claude -p /usage`
  plus in-stream `rate_limit_event` (five_hour, seven_day, seven_day_opus, seven_day_sonnet), never
  a direct Anthropic call; Copilot `copilot_internal/user`; Cursor `GetCurrentPeriodUsage`; Grok
  `v1/billing?format=credits`; Gemini/agy `retrieveUserQuota`; API-key balances for DeepSeek,
  Moonshot, OpenRouter, SiliconFlow, StepFun.
- aid gaps: snapshot fields are label/used/resets_at/group/plan only, so a Codex account at 100%
  that still serves on credits reads as held; `live_quota_refresh.rs:22-30` `MAPPED` omits Droid
  (verified); no source for Gemini, Copilot, CommandCode, Kilo, MiMoCode, Oz.
- Applicable mechanism: schema fields `credits`, `reset_credits`, `held`, `span_secs` in aidbar and aid
  together; a reset reminder (window ≥24h renewing soon with <85% used). Not applicable: writing
  refreshed tokens back into a CLI's auth file; spending reset credits automatically.

## 5. Running a CLI on another vendor

- Magpie `provider/presets.go:102-486` lists per vendor its Chat, Responses and Anthropic
  endpoints, and separate plan vs pay-as-you-go endpoints (Zhipu/Z.ai coding, Kimi Code, StepFun
  step_plan, Volcengine coding and plan, Tencent plan, Xiaomi token-plan, Qianfan tokenplan,
  Huawei plan). A plan key is refused at the pay-as-you-go endpoint. No vendor speaks the Gemini
  protocol, so gemini cannot be repointed.
- Wiring: claude reads `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`,
  `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL`, `CLAUDE_CODE_SUBAGENT_MODEL`, and for unknown
  models `CLAUDE_CODE_MAX_CONTEXT_TOKENS` / `CLAUDE_CODE_MAX_OUTPUT_TOKENS`; codex takes a
  `[model_providers.X]` table with `base_url` and `wire_api` (`model_provider` without its table
  makes the config unloadable); opencode uses `@ai-sdk/openai-compatible`.
- aid: `Route::via()` has no caller; provider, egress, quota and attribution key on the CLI.
- Applicable mechanism: a vendor-protocol table, route synthesis only where the CLI's protocol is served,
  keys resolved at spawn time from a declared variable name and never placed in argv or persisted
  files, `ProviderId` per plan, egress from the resolved URL.

## 6. Live model catalog

- Magpie (`catalog/catalog.go`): models.dev with a majority vote per bare id; lookups fall back
  through bare id, region prefix, `:tag`, `.`≡`-`; dated ids map to aliases
  (`provider/group.go:426-461`, table in `modelsame_test.go`); `[1m]` stripped; a missing cache is
  unknown, never empty. Codex `models_cache.json` entries with `visibility=hide` are skipped.
- aid (verified): `agent/selection_scoring.rs:45` rating lookup is exact `==`;
  `cost/pricing_resolution.rs:13-18` resolves static rows before the live feed;
  `agent/codex.rs:196-212` keeps every slug and drops reasoning levels and context.
- Applicable mechanism: one `canonical_model_id` normaliser used by rating, gate, pricing and probes; live
  vendor price before static rows for vendor CLIs; Codex probe filter; context, reasoning levels
  and release date from models.dev. Capability ratings stay hand-curated.

## 7. Multiple accounts per CLI

- Magpie: accounts in `logins.json` (0600, atomic, stable id). Identity is Claude
  `(email, organizationUuid)` and Codex `(email, chatgpt_account_id)`; two Team seats share an
  email, so display-name matching once deleted both. Each Claude account has its own
  `CLAUDE_CONFIG_DIR` and keychain item `Claude Code-credentials-<sha8(dir)>`, signed in by
  running the CLI's own login there. A refresh token has exactly one holder; credentials are never
  copied between directories.
- aid: one sign-in per CLI; `CODEX_HOME` is overwritten at launch; `Route` and markers have no
  account. The aid state dir is not in the isolated-HOME denylist, so per-account directories
  would need a location outside it.
- Applicable mechanism: account as a route dimension, per-account markers first.

## 8. Token and cost accounting

- aid (verified): `cost/mod.rs:48-52` `estimate_cost` prices every token at 0.7×input+0.3×output.
  Codex `tokens` include cached input, so a turn of 2.0M input (1.8M cached) and 20k output at
  $1.25/$0.125/$10 estimates ~$7.8 against ~$0.68 actual. `watcher.rs:249-274` overwrites tokens
  and cost on every Completion event, and OpenCode emits one per `step_finish`
  (`agent/opencode.rs:100`), so a multi-step run records its last step and `max_task_cost`
  compares against one step. serde_json is built without `preserve_order` (`cargo tree`: features
  `default`, `std`), so `modelUsage.keys().next()` is alphabetical: an Opus run that also used
  Haiku records Haiku.
- Magpie (`sessions/`): Claude usage per assistant message, deduplicated by message id plus outer
  `requestId`; cache writes split 5m/1h; Codex `token_count` totals with counter epochs across
  resume and compaction; OpenCode child sessions rolled into the parent; prices with cache read,
  cache write, 1h write and context tiers; unpriced models counted, not zeroed.
- Applicable mechanism: a `Usage {input_uncached, output, cache_read, cache_write, cache_write_1h, reasoning}`
  and `cost_of(Usage)`; breakdown columns on the task row; summed OpenCode steps; the
  highest-usage model from `modelUsage`. Codex resume double counting and subagent coverage need
  real fixtures before any change.

## 9. Per-CLI config and home variables

- Magpie `agentenv/agentenv.go:30-72` is the single list of per-CLI config/home variables
  (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `CODEX_SQLITE_HOME`, `GEMINI_CLI_HOME`, `OPENCODE_CONFIG_DIR`,
  `OPENCODE_DB`, `COPILOT_HOME`, `CURSOR_CONFIG_DIR`, `GROK_HOME`, `FACTORY_HOME_OVERRIDE`,
  `MIMOCODE_HOME`, `PI_CODING_AGENT_DIR`, …) with `NotPaths`, a test that every listed variable is
  read, and a test harness (`testenv`) that redirects HOME/XDG, puts failing stand-ins for the
  agent CLIs and `security` first on PATH, and refuses to run if lookups escape the temp dir.
- aid keeps three separate home-dir lists (`agent/home_isolation.rs` `DEFAULT_DENYLIST`,
  `sandbox.rs`, `container.rs`) with no agreement test, and has no per-CLI variable inventory.
- Applicable mechanism: one inventory table that derives the three lists, an explicit launch policy per
  variable, the "variable is read" and "lists agree" tests, and stand-in binaries in tests.

## 10. Early failover and route exhaustion

- Magpie holds a reply until first real content and fails over silently before it
  (`gateway/fallback.go` `holdWriter`); when every candidate fails it shows the first non-quota
  error; `magpie quota wait` polls at the soonest reset +30 s within 1–10 min (exit 0/1/2/130).
- aid (research finding): a non-quota failure runs `prepare_retry` (5/15/45 s backoff, consumes
  `--retry`) before `--cascade`; a silent first-token hang already cascades without consuming
  `--retry`; when every route is held the error names the first one only and a background wait
  watches that one agent; a cascade after a mid-task death reuses the prompt without mentioning
  the salvaged WIP commit.
- Applicable mechanism: cascade immediately on a fast failure that produced no work; list every held route
  with its reset and wait on the soonest; summarise the chain leading with the first non-quota
  error; tell the next agent a WIP commit exists. Not applicable: SSE keepalives, automatic
  reset-credit spending.

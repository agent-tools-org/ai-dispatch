# Model catalog audit — what providers actually serve vs what aid claims

Date: 2026-08-05. Method: per `docs/design/cli-adapter-audit.md`, every claim below comes from
captured output of the installed CLI, a config file the CLI reads, or a probe run. Nothing is
taken from aid's source except the catalog claims column (which is what is being audited).
Sources: `src/model_catalog.rs` `AGENT_MODELS` / `AGENT_PROFILES` (the claims).

Probe inventory: codex-cli 0.145.0, gemini 0.43.0, agy 1.1.10, qwen 0.21.5, opencode 1.18.1,
kilo 7.0.47, cursor-agent 2026.07.23-e383d2b, copilot 1.0.77, droid 0.183.0, oz (Warp),
claude 2.1.222, codebuff 1.0.685. mimocode: not installed.

## Cross-cutting findings

1. **The gpt-4.1 hardcoding report is not reproducible at HEAD.** `pricing_builtin::for_model_lower`
   returns `None` for unknown models; callers `cmd/stats.rs:task_cost` and `cmd/cost.rs:task_cost`
   do `.unwrap_or(0.0)`. Net effect: tasks attributed to unknown models are costed **$0.00**, not
   gpt-4.1 rates. (If the report predates recent cost fixes, it is stale; either way the failure
   mode at HEAD is silent zero-costing of unknown models.)
2. **`AGENT_PROFILES` has no row for Oz** even though `AgentKind::Oz` exists and `oz model list`
   returns 89 models. Oz also has zero `AGENT_MODELS` entries; cost resolution returns `None`.
3. **Copilot and Antigravity have profile rows but zero model entries**, while `agy models`
   returns 11 live models.
4. Coverage: opencode catalog covers 5 of 177 served models (2.8%), kilo 1 pseudo-entry of 432,
   cursor 5 entries vs ~195 served (3 stale), codex 3 of 7 listed served models.

## codex (codex-cli 0.145.0, ChatGPT-account login)

Evidence: `~/.codex/models_cache.json` (fetched_by CLI `2026-08-05T11:48:45Z`);
`~/.codex/config.toml` `model = "gpt-5.6-sol"`; `auth.json` `auth_mode=chatgpt`, id-token claim
`chatgpt_plan_type="prolite"` (token values not reproduced).

| Served now (models_cache.json, visibility=list) | In aid catalog? | Catalog tier/price ($/M in/out) |
|---|---|---|
| gpt-5.6-sol (**configured default**) | NO | — |
| gpt-5.6-terra (upgrade target of gpt-5.4) | NO | — |
| gpt-5.6-luna (upgrade target of gpt-5.4-mini) | NO | — |
| gpt-5.5 | yes | premium 2.5/15, cap 9.6 |
| gpt-5.4 | yes | premium 2.5/15, cap 9.3 |
| gpt-5.4-mini | yes | cheap 0.4/1.6, cap 7.0 |
| gpt-5.3-codex-spark (api=false) | NO | — |

Divergences: entire gpt-5.6 generation missing; `config.toml` migration note
`"gpt-5.3-codex" = "gpt-5.4"` confirms churn. Pricing: `pricing_builtin` has no gpt-5.6 branch, so
gpt-5.6-* falls through to generic `gpt-5` at $1.25/$10 while gpt-5.5 is $2.5/$15 — the current
flagship family is under-priced. `codex_fallback_pricing` finds no "standard" tier (catalog has
premium/cheap only) and falls back to first entry gpt-5.5, so unknown-model codex tasks cost at
the premium blend (~$6.25/M).

## gemini (0.43.0) — account LOCKED OUT

Evidence, probe run `gemini -p "Reply with exactly: ok" -o json --yolo`:
`Error authenticating: IneligibleTierError: This client is no longer supported for Gemini Code
Assist for individuals. To continue using Gemini, please migrate to the Antigravity suite...`
with `ineligibleTiers: [{reasonCode:'UNSUPPORTED_CLIENT', tierId:'free-tier',
tierName:'Gemini Code Assist for individuals'}]`. Settings: `selectedType: "oauth-personal"`,
no GEMINI_API_KEY set.

| aid catalog claims (8 entries) | Served to this account |
|---|---|
| flash, pro, flash-lite (aliases) | nothing — auth rejected |
| gemini-3.1-pro-preview, gemini-3-flash-preview, gemini-3-flash-lite-preview | nothing |
| gemini-2.5-flash, gemini-2.5-pro | nothing |

Divergence: every gemini dispatch on this machine fails before model selection, but the catalog
still presents gemini as an eligible cheap/premium provider. Note the gemini generation names now
live in agy (gemini-3.6/3.5-flash, gemini-3.1-pro), not in these catalog names.

## agy / Antigravity (1.1.10)

Evidence, `agy models`:

| Served now | In aid catalog? |
|---|---|
| gemini-3.6-flash-high / -medium / -low | NO |
| gemini-3.5-flash-high / -medium / -low | NO |
| gemini-3.1-pro-high / -low | NO |
| claude-sonnet-4-6, claude-opus-4-6-thinking | NO |
| gpt-oss-120b-medium | NO |

Divergence: `AGENT_MODELS` has **zero** Antigravity entries (profile row only, "free (Google One
/ Gemini Code Assist) or BYOK"). `resolve_pricing(Antigravity)` returns None → all agy tasks show
"—" cost; budget/selection logic has nothing to pick from.

## qwen (0.21.5, Bailian Token Plan via BAILIAN_TOKEN_PLAN_API_KEY)

Evidence:
- Plan endpoint (the CLI's own baseUrl) `GET .../token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1/models`
  → **12 models**: qwen3.8-max, qwen3.8-max-preview, qwen3.7-max, qwen3.7-plus, qwen3.6-flash,
  glm-5.2, deepseek-v4-pro, deepseek-v4-flash-0731, qwen-audio-3.0-realtime-plus,
  qwen-audio-3.0-tts-plus, wan2.7-image, wan2.7-image-pro.
- `~/.qwen/settings.json` lists 17 `modelProviders.openai` ids + selected model `qwen3.8-max`.
- Probe `qwen -p ... -m qwen3.8-max` → success envelope `"model":"qwen3.8-max"`.
- Probe `-m qwen3.6-flash` → success. Probe `-m MiniMax-M2.5` →
  `[API Error: 403 Access to model denied. Please make sure you are eligible for using the model.]`

| settings.json id | Served by plan (/v1/models)? | Probe result |
|---|---|---|
| qwen3.8-max (selected) | yes | OK |
| qwen3.8-max-preview, qwen3.7-max, qwen3.7-plus, qwen3.6-flash, glm-5.2, deepseek-v4-pro, deepseek-v4-flash-0731 | yes | qwen3.6-flash OK |
| qwen3.6-plus, deepseek-v4-flash, deepseek-v3.2, kimi-k2.7-code, kimi-k2.6, kimi-k2.5, glm-5.1, glm-5 | **NO** | — |
| MiniMax-M2.5 | **NO** | 403 captured |

Divergences: (1) aid's dynamic qwen catalog (`load_qwen_models`, settings.json) carries **9 models
the plan does not serve** — any selection of them 403s; `/v1/models` is the correct source.
(2) All dynamic entries are hardcoded tier="free", price 0.0, capability 7.4 — the plan is a paid
subscription, and a plan-served flash model is not equivalent to a frontier model. (3) Plan serves
4 media/audio models absent from settings.json (not usable via qwen CLI chat; noted for
completeness). (4) Static fallback entry "coder-model" survives in error paths; it names no real
model. HEAD's `budget_model` fix (prefer selected model) currently selects qwen3.8-max, which is
served — the breakage returns whenever a different dynamic entry is chosen.

## opencode (1.18.1)

Evidence: `opencode models` → 177 model ids; `~/.config/opencode/opencode.json` defines custom
provider `mimo` (Xiaomi token plan) with mimo-v2.5, mimo-v2.5-pro.

| aid catalog entry | Exists in `opencode models`? |
|---|---|
| opencode/glm-5.2 (cheap 0.38/1.98) | yes |
| opencode/kimi-k2.6 (cheap 0.45/2.20) | yes |
| opencode/deepseek-v4-flash-free | yes |
| opencode/nemotron-3-ultra-free | yes |
| opencode/mimo-v2.5-free | yes |

Divergence: all 5 entries exist, but catalog covers 5/177; none of opencode's current flagships
(opencode/claude-opus-5, opencode/gpt-5.6-*, opencode/gpt-5.5-pro, opencode/claude-sonnet-5...)
are listed, so selection/costing for them falls to substring pricing only.

## kilo (7.0.47)

Evidence: `kilo models` → 432 ids; 12 free variants, incl. `kilo/kilo-auto/free`,
`kilo/openrouter/free`, `kilo/nvidia/nemotron-3-ultra-550b-a55b:free`.

| aid catalog entry | Reality |
|---|---|
| `default` (free, cap 3.8) | No model id `default` exists. Free auto routing is `kilo/kilo-auto/free`. |

Divergence: single pseudo-entry; the real free default id is `kilo/kilo-auto/free`. (Related:
`pricing_builtin` tests reference `kilo/kilo/auto-free`; the free match still works by substring
`kilo`+`free`, but neither string is the real id.)

## mimocode — NOT INSTALLED (honest gap)

`mimocode` binary absent; cannot probe. Catalog entry `mimo/mimo-auto` does not appear in the
shared opencode registry listing (`opencode models` shows only `mimo/mimo-v2.5`,
`mimo/mimo-v2.5-pro`), suggesting the entry is stale — marked UNVERIFIED, no CLI to exercise.

## cursor (cursor-agent 2026.07.23)

Evidence: `cursor-agent --list-models` → 195 models; default line: `auto - Auto (current, default)`.

| aid catalog entry | Exists? | Notes |
|---|---|---|
| composer-2 (standard 0.50/2.50, "default", cap 8.5) | **NO** | superseded by `composer-2.5` / `composer-2.5-fast` |
| auto (cheap, cap 7.0) | yes | actual CLI default |
| composer-1.5 (standard, cap 8.0) | **NO** | gone from list |
| opus-4.6-thinking (premium, cap 9.2) | **NO** | real ids: `claude-4.6-opus-high-thinking`, `claude-4.6-opus-max-thinking` |
| gpt-5.4-high (premium, cap 9.0) | yes | served |

Divergence: 3 of 5 entries stale; the entry labeled "(default)" no longer exists, and the real
default `auto` is present. List also shows generations the catalog lacks entirely
(claude-opus-5*, claude-fable-5*, gpt-5.6-sol/terra/luna, gpt-5.3-codex*, kimi-k3, grok-4.5...).

## copilot (1.0.77) — cannot enumerate (honest gap)

Evidence: `copilot --help` subcommands = completion/help/init/login/mcp/plugin(s)/skill/update/
version — **no models command**; `~/.copilot/settings.json` is `{}`; `~/.copilot/config.json`
carries only firstLaunchAt/trusted_folders. Auth state not determinable from disk; a probe would
require `--allow-all` and consume premium requests, so not run.
Catalog: zero model entries (profile row "subscription" only). Status: UNVERIFIED — no CLI path
to enumerate served models found.

## droid (0.183.0) — partial

Evidence: `droid exec --help`: `-m, --model <id>  Model ID to use (default: claude-opus-5)`.
No model-list subcommand exists; `~/.factory/settings.json` holds no model config.

| aid catalog entry | Reality |
|---|---|
| sonnet (standard 3/15, "default", cap 8.5) | CLI default is **claude-opus-5**, not sonnet |
| opus (premium 15/75) | alias style plausible, full id list UNVERIFIED |
| haiku (cheap 0.25/1.25) | UNVERIFIED (no list command) |

Divergence: catalog "(default)" label contradicts captured CLI default. Full served set cannot be
captured from this CLI.

## oz (Warp)

Evidence: `oz model list --output-format text` → 89 models: auto, auto-efficient, auto-genius,
auto-open; claude-4-5-{haiku,opus,sonnet}(+thinking); claude-4-6/4-7/4-8-opus/sonnet tiers;
claude-5-{fable,opus,sonnet} tiers; deepseek-v4-pro-fireworks; gemini-3.1-pro, gemini-3.5-flash,
gemini-3.6-flash; glm-5.2-fireworks; gpt-5-2/5-3-codex/5-4/5-5/5-6-{sol,terra,luna} tiers;
grok-4-3/4-5; grok-build-0.1; kimi-k26/k27-code/k3-fireworks; minimax-2.7/3-fireworks;
qwen-3.6/3.7-plus-fireworks.

Divergence: **AGENT_PROFILES and AGENT_MODELS contain no Oz rows at all**; every oz task is
uncosted (resolve_pricing → None) and invisible to model-based selection.

## claude (Claude Code 2.1.222)

Evidence: probe `claude -p "Reply with exactly: ok" --output-format json` → result envelope:
`"modelUsage":{"claude-opus-5[1m]":{... "costUSD":0.204351..., "canonicalModel":"claude-opus-5",
"provider":"firstParty"}}` (6 in, 571 out, 74,973 cache-read, 15,256 cache-write tokens);
`~/.claude/settings.json` `"model": "opus[1m]"`; OAuth org account, no ANTHROPIC_API_KEY.
No model-list command found in `claude --help`.

| aid catalog entry | Reality |
|---|---|
| sonnet (3/15, cap 8.8) | alias scheme matches CLI (`opus[1m]` style); resolution UNVERIFIED |
| opus (15/75, cap 9.4) | resolves to **claude-opus-5**; 15/75 are opus-4-era rates, not CLI-verifiable |
| haiku (0.8/4, cap 6.2) | UNVERIFIED |

Divergence: canonical model moved to claude-opus-5; catalog prices (15/75) not confirmable from
CLI. Observed opus-5[1m] envelope cost $0.2044 for ~90.8k tokens (cache-dominated) is captured
above as the only first-party pricing evidence.

## codebuff (1.0.685) — cannot probe (honest gap)

Evidence: no credential store found (`~/.codebuff`, `~/.config/codebuff`,
`~/Library/Application Support/codebuff*` all absent); CLI exposes only `login`/`publish`
subcommands plus `--lite/--max/--plan` modes, no model flag or list. Catalog entry:
`auto` (standard, "SDK-managed pricing"). Status: UNVERIFIED — no credentials, no listing path.

## Ranked defects

1. **cursor: 3/5 catalog entries dead**, including the one labeled default (composer-2, composer-1.5, opus-4.6-thinking).
2. **qwen dynamic catalog inherits 9 unserved models** from settings.json; only `/v1/models` reflects the plan (12 models).
3. **gemini: account ineligible** (IneligibleTierError) yet catalog/selection present it as available.
4. **codex: gpt-5.6 generation absent**; configured default gpt-5.6-sol unknown to the catalog and priced at generic gpt-5 rates.
5. **Oz: no catalog presence at all** despite 89 served models.
6. **Antigravity: zero entries** despite 11 served models (all agy tasks uncosted).
7. **kilo: `default` pseudo-entry**; real free id is `kilo/kilo-auto/free`.
8. **droid: "(default)" label contradicts CLI default claude-opus-5**.
9. **claude: opus-5 pricing unverifiable from CLI**; catalog still at opus-4-era 15/75.
10. **cost fallback for unknown models is $0.00**, not gpt-4.1 (reported defect is stale; zero-costing is the real behavior).

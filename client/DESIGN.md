# AID Command — desktop client design spec

Authoritative spec for `client/`. A game-styled console for `aid`, running as a
**native macOS app and an iPadOS app from one source tree**. You watch a fleet of
dispatched agents from a bridge, not from a task list.

Visual source of truth: the `AID Command v3` design canvas (claude.ai/design project
`8492447e-8642-45aa-8b86-936802717622`), a 1440×900 bridge layout. Every token,
color, label and demo record below is transcribed from it.

Server contract: `docs/design/client-api.md`. The client never invents a field the
server does not send.

---

## 1. Vocabulary

The app renames aid's domain into fleet language. This mapping is fixed; use it in
all user-facing strings.

| aid concept | In-app name |
|---|---|
| project / repo | SECTOR |
| task | MISSION |
| task id | mission id (`t-…`, shown verbatim) |
| agent (codex, cursor, …) | UNIT / CREW member |
| model | drive |
| workgroup | WG-`<id>` |
| deliverable / diff / report | PAYLOAD (CARGO hold) |
| cost | FUEL SPENT |
| rate limit / quota | QUOTA lamp |
| worktree | BAY (HANGAR) |
| declared difficulty | THREAT (I–V) |

## 2. Status model

Four display states. Everything the API reports collapses into one of them.

| Display | Label | Color | Mark | aid `status` values |
|---|---|---|---|---|
| RUN | RUNNING | `#9FD8CD` | `▶` | `running`, `waiting`, `pending`, `awaiting_input`, `stalled` |
| DONE | COMPLETE | `#8FBE79` | `✦` | `done`, `merged` |
| FAIL | LOST | `#D4826F` | `✕` | `failed` |
| STOP | HELD | `#E0B15E` | `⏸` | `stopped`, `skipped` |

`awaiting_input` and `stalled` are RUN but need the commander: raise an attention
lamp on the row and show `awaiting_reason`. Do not invent a fifth color.

Verification is a **separate axis**. When `outcome` is not `verified`/`delivered`/
`in_progress`, show a small tag beside the state pill: `VFAIL`, `VTIMEOUT`, `VINFRA`,
`VNORESULT`, `BROKEN`. Never fold outcome into the state color — a mission can be
COMPLETE and unverified, and the console must show both at once.

## 3. Layout

The canvas layout is the **Mac window layout**, essentially 1:1. Min window
1180×760, ideal 1440×900.

```
┌────────────────────────────────────────────────────────────────────┐
│ HUD BAR  mark · title · RANK+XP · CONDITION · tabs · clock          │  62pt
├────────────────────────────────────────────────────────────────────┤
│ GAUGE STRIP  6 cells · KLAXON                                       │  96pt
├──────────────┬─────────────────────────────────────────────────────┤
│ LEFT RAIL    │  CENTER — FLEET LOG | HANGAR | CARGO                 │
│ tactical     │                                                     │
│ scan (radar) │                                                     │  flex
│ + SECTORS    │                                                     │
├──────────────┴─────────────────────────────────────────────────────┤
│ MISSION BRIEF  unit card · crew · brief+log · stats/payload/actions │  268pt
└────────────────────────────────────────────────────────────────────┘
```

Left rail 290pt fixed. Bottom brief is a fixed band on Mac, collapsible by
dragging its divider.

**iPad landscape** uses the same three bands; the left rail narrows to 244pt.
**iPad portrait** (and any width < 1000pt) collapses: the left rail becomes a
slide-over invoked from the HUD, and the Mission Brief becomes a pushed detail
sheet instead of a bottom band. One layout decision, expressed as a single
`ConsoleLayout` size class — not scattered `if idiom == .pad` checks through the
screens.

### 3.1 HUD bar

- ship mark (rotated square, accent, pulsing lamp) + `AID · FLEET COMMAND`
- sub-line `BUILD <server.version> · SECTORS <n> · CMDR <name>`
- RANK `<I…VIII>` + XP bar (`xp % 1000` of 1000)
- CONDITION lamp: GREEN (0 lost) / AMBER (1–3) / RED (≥4), from `summary.failed`
- tab group: `FLEET LOG` `HANGAR` `CARGO`, clipped-corner tabs
- clock, mono, HH:MM:SS
- link indicator: green when SSE heartbeats arrive, red after 3 missed

### 3.2 Gauge strip

Six cells, each a lamp dot + mono label + big value + 8 pips, all from
`/api/fleet` `summary`:

| Gauge | Value | Color |
|---|---|---|
| DRIVES | `running` | run |
| CLEARED | `done` | done |
| LOST | `failed` | fail |
| FUEL SPENT | `$<spend_usd>` | stop |
| REACTOR | derived agent load | ink |
| MEMORY | `memory_mb` MB | ink |

The strip must state its window (`summary.window`) in small type — a count with no
window reads as a total and would be a lie.

### 3.3 Left rail

- TACTICAL SCAN: a 128pt circular scope, one blip per mission, angle from a hash of
  the id, radius from remaining progress, color from state, RUN blips pulsing.
  Header shows `<n> CONTACTS`.
- SECTORS list: tag, name, `<done>✦ <run>▶ <fail>✕`, and a segment bar (one
  segment per mission, colored by state). Selecting a sector drives HANGAR.

### 3.4 FLEET LOG (center tab 1)

Collapsible sector groups. Sector header: tag, name, `WG-<UPPER>`, `<done>/<total>
CLEARED`, segment bar.

Mission row columns, in this order:
`mark │ MISSION (title + "<id> · THREAT <n>") │ UNIT (glyph + agent, drive glyph +
model) │ STATUS (pill) │ PROGRESS (14 segments + %) │ ELAPSED │ TOKENS │ COST`

A row that transitions to DONE/FAIL flashes once (green wash → transparent, 900ms)
and fires a toast. Selected row gets accent brackets on both edges.

### 3.5 HANGAR (center tab 2)

Bay grid for the selected sector — Mac 4 columns, iPad landscape 3, narrow 2.
Each bay card: `BAY <n>`, state pill, large percent readout in a themed frame,
mission title, then unit glyph + agent + drive glyph + elapsed + cost.
Empty bays render as outlined placeholders with hazard stripes.
Header: sector tag, name, `WG-… · WORKTREE ISOLATION ON`, `<done>/<total> BAYS CLEARED`.

### 3.6 CARGO (center tab 3)

Header `Cargo hold` + `<n> STOWED · <n> CRITICAL`. Filter chips `ALL`, `RELEASE`,
`REPORT`, `PATCH`, `AUDIT`, `BENCH`, `FIXTURE`, `DATASET`, `SCRAP` with counts.
Payload row: name + mission id, kind tag, priority chevrons (`▰▰▰▱▱`) + label,
sector, manifest (`<stat1> · <stat2>`).

Priority table — drives chevrons, color and XP:

| key | label | color | rank | xp |
|---|---|---|---|---|
| legendary | CRITICAL | `#8FBE79` | 5 | 320 |
| epic | HIGH | `#E0B15E` | 4 | 220 |
| rare | STANDARD | `#A7A6A0` | 3 | 140 |
| uncommon | LOW | `#A7A6A0` | 2 | 90 |
| common | ROUTINE | `#7A7974` | 1 | 50 |
| salvage | SALVAGE | `#D4826F` | 1 | 20 |

A FAIL or STOP mission always yields `SCRAP` / "Salvaged worktree" / salvage — aid
preserves the worktree, so the payload is never nothing.

Payload kind and priority are **derived client-side** from what the server reports
(`has_diff`, `has_result`, tokens, cost, outcome). This is presentation, and the
derivation lives in one file so it can be replaced when the server learns to
classify deliverables itself.

### 3.7 MISSION BRIEF (bottom band / detail)

- unit card: large agent glyph, agent name, `<ROLE> · LV <n>`, drive chip, star row,
  `MISSIONS FLOWN <n>` (from `/api/agents` `task_count`)
- CREW list: every agent with a busy/ready dot and `ENG`/`RDY`, plus its quota lamp
- brief: state pill, `THREAT <n>`, `<id> · <agent> · <model>`, title, the prompt in a
  mono block, then the latest events
- stats row: ELAPSED / MEMORY / TOKENS / COST
- PAYLOAD strip: kind + name + chevrons
- lamp row: LINK, DRIVE, SHIELD, QUOTA, DOCK
- action bar: `ABORT` `RELAUNCH` `STEER` `DIFF` `EXPORT` `DOCK` — DOCK is the filled
  primary. Wiring, per `docs/design/client-api.md`:
  ABORT→`POST /stop`, RELAUNCH→`POST /retry` (feedback sheet), STEER→`POST /steer`
  (message sheet), DIFF→`GET /diff` (viewer), EXPORT→`GET /result` (save panel),
  DOCK→`POST /merge`. An action the server rejects with `409` shows the server's
  reason inline; the client does not retry and does not fake success.
  RELAUNCH and DOCK are destructive-ish and ask for confirmation; ABORT does not.

### 3.8 SETTINGS

A window (Mac: `⌘,`, a Settings scene) / a sheet (iPad):
theme picker with large live previews, server host + port + token (token stored in
the Keychain, never in UserDefaults), Demo/Live source, sound, haptics, reduce-motion
override, commander name.

Include a **Connect** flow: enter `http://<mac-ip>:8080` and the token printed by
`aid web --host 0.0.0.0`, then a Test button that reports the real result of a
`/api/fleet` call — status code and message, not a green tick.

### 3.9 Mac affordances

Real ones, not iPad-in-a-window:
- menu bar: File (Connect…, Disconnect), View (theme switch, tab switch, toggle
  brief band), Mission (Abort/Relaunch/Steer/Diff/Dock on the selection)
- keyboard: `⌘1/2/3` tabs, `⌘T` theme, `⌘R` refresh, `↑/↓` move selection,
  `⌘⌫` abort, `Space` peek at the brief
- hover states are live on Mac and pointer-attached iPad only
- window restoration: selected sector, tab and theme survive relaunch

## 4. Theme system

**Two themes: `starship` and `pixel`.** Same layout, same screens, same primitives —
only tokens and a few themed shapes change.

> Red line: no view outside `Theme/` and `Primitives/` may branch on the active
> theme. If a screen needs `if theme == .pixel`, the missing abstraction belongs in
> a primitive instead.

### 4.1 Token contract

```swift
protocol ThemeTokens {
    // color
    var bgDeep: Color { get }      // window floor
    var bg: Color { get }          // panel interior
    var panelEdge: Color { get }   // structural line
    var ink: Color { get }
    var ink2: Color { get }
    var ink3: Color { get }
    var accent: Color { get }
    var run: Color { get }
    var done: Color { get }
    var fail: Color { get }
    var stop: Color { get }
    // type
    func font(_ role: TypeRole) -> Font
    // geometry
    var panelCut: CGFloat { get }
    var hairline: CGFloat { get }
    var pipSize: CGSize { get }
    var spacing: SpacingScale { get }
    // texture + motion
    var overlay: TextureStyle { get }   // .scanline | .dither | .none
    var motion: MotionProfile { get }
}
```

### 4.2 The two themes

| Token | `starship` | `pixel` |
|---|---|---|
| bgDeep | `#0A0B09` under a radial `#22251F → #101210 → #0A0B09` | `#0F0F1B` flat, no gradient |
| bg (panel) | `#1B1C19` | `#242438` |
| panelEdge | `#2C2C29`, hairline 1pt | `#5A5A8C`, hard 2pt, no anti-aliasing |
| ink / ink2 / ink3 | `#ECEBE7` / `#A7A6A0` / `#7A7974` | `#F4F4F8` / `#B8B8D0` / `#7A7A9E` |
| accent | `#E0B15E` amber | `#F8D048` sunflower |
| run / done / fail / stop | `#9FD8CD` / `#8FBE79` / `#D4826F` / `#E0B15E` | `#3CE0D0` / `#5CD44C` / `#F04848` / `#F8D048` |
| panel shape | clipped corners, 16–20pt bevel: `(0,0) (W-20,0) (W,20) (W,H) (20,H) (0,H-20)` | stepped 4pt staircase corner drawn as squares — never a radius |
| body type | system, default design | system `.monospaced`, `.bold`, tracking +0.5 |
| label type | `.monospaced` 11pt, tracking 0.14em, uppercase | `.monospaced` 10pt, tracking 0.10em, uppercase |
| progress | 14 thin segments, 3pt gap, leading segment glows | 14 chunky 8pt blocks, 2pt gap, no glow, leading block blinks on 2 frames |
| lamp | round, soft brightness pulse | square 6×6, hard 2-frame on/off |
| texture | scanlines (`black 22%`, 1pt every 4pt) + accent grid every 72pt + slow vertical sweep | 2×2 checkerboard dither at 6%, no sweep |
| screen corners | accent L-brackets, 30×30, 2pt | four 8×8 block sprites |
| motion | eased, 1.4–7s loops | stepped, quantized to 8 fps, no easing |
| sfx | triangle wave | square wave, same notes |

Both themes are dark. No light variant. Theme changes crossfade in 240ms and must
not reload data or lose selection.

### 4.3 Primitives — the only theme-aware views

`ThemedPanel` · `PanelShape` · `StatusLamp` · `SegmentBar` · `StatePill` ·
`MonoLabel` · `GaugeCell` · `ThemedButton` · `TextureOverlay` · `ScreenFrame` ·
`ContactScope` (radar in starship, blocky minimap in pixel) · `RarityChevrons` ·
`UnitGlyph` · `ToastView`.

### 4.4 Fonts

No network fonts, no packages. `Font.system(..., design: .monospaced)` for both
themes. A bundled pixel face, if ever wanted, must be an OFL file committed under
`client/AIDCommand/Resources/Fonts/` — never fetched at build time.

## 5. Unit glyphs

Canvas SVG paths, ported to SwiftUI `Path` (24×24 box, stroke 1.7, round caps/joins):

| agent | role | lv | path |
|---|---|---|---|
| codex | BUILDER | 7 | `M9 4 L3 12 L9 20 M15 4 L21 12 L15 20 M12 7 L12 17` |
| cursor | PILOT | 5 | `M5 3 L19 11 L12 12.5 L9.5 20 Z` |
| grok | AUDITOR | 9 | `M3 12 C7 7 17 7 21 12 C17 17 7 17 3 12 Z M12 9.6 A2.4 2.4 0 1 0 12 14.4 A2.4 2.4 0 1 0 12 9.6` |
| gemini | SCOUT | 4 | `M12 2 L14 10 L22 12 L14 14 L12 22 L10 14 L2 12 L10 10 Z` |
| opencode | ENGINEER | 6 | `M12 3 L20 7.5 L20 16.5 L12 21 L4 16.5 L4 7.5 Z M12 8.5 L15.5 10.5 L15.5 14 L12 16 L8.5 14 L8.5 10.5 Z` |
| kilo | SAPPER | 3 | `M4 19 L12 5 L20 19 M8.5 19 L12 12 L15.5 19` |

Unknown agents fall back to the kilo glyph — and aid has more agents than these six
(qwen, droid, agy, oz, copilot, commandcode, mimocode, claude, goose…), so the
fallback is the common case, not an edge case. Give it a neutral hexagon rather than
a misleading role. Roles and levels for unlisted agents are unknown: render `—`.

Drive (model) glyphs key off a regex on the model name: `gpt` → hexagon,
`grok` → arrow, `gemini` → sparkle, `glm` → three bars, `composer` → ringed dot,
`auto` → hollow circle, else a square.

In `pixel` the same paths render through a quantizing modifier (snap to a 2pt grid)
so they read as sprites — one primitive, two renderers.

## 6. Data layer

```swift
protocol FleetDataSource: Sendable {
    var stream: AsyncStream<FleetSnapshot> { get }
    func detail(_ id: MissionID) async throws -> MissionDetail
    func diff(_ id: MissionID) async throws -> String
    func result(_ id: MissionID) async throws -> String
    func act(_ action: MissionAction, on id: MissionID) async throws
}
```

Two implementations:

- **`DemoSource`** — the canvas dataset (§7) with a 1s tick that advances running
  missions, completes one every 13 ticks and loses one every 29 ticks, awarding XP
  and firing toasts. This is what previews and first launch use, and what lets the
  client be built before the server endpoints land. Deterministic apart from the tick.
- **`LiveSource`** — `aid web`, per `docs/design/client-api.md`: `GET /api/fleet` for
  the snapshot, SSE `/api/events` for updates, the action endpoints for the brief's
  buttons. Bearer token from the Keychain.

Connection states are explicit and visible: `disconnected`, `connecting`,
`live`, `degraded` (heartbeats missed, showing stale data with its age), `error`
(with the server's own message). Never silently show stale data as live.

> Unknown stays unknown. A `null` cost renders `—`, never `$0.00`. A `null` model
> renders `—`, never `auto`. An undeclared profile field renders blank, never a
> default. This is a standing rule in this repo and the client is not exempt.

Progress for a running mission has no server value. Derive it from elapsed against
the median duration of that agent's completed missions, clamp to 0.97, and never
show 100% for a mission that is still RUN. Put the derivation in one file.

## 7. Demo dataset

Transcribed from the canvas; keep ids, titles, durations, tokens and costs exactly
so the app matches the design.

- **SEC-01 `uniswapx-filler`** (wg `8937e74c`): `t-561886a0` codex/gpt-5.6-sol RUN 0.42 III
  "Advisory sweep of the filler quote path" 18m 04s / 1.4M / — / 145M ·
  `t-1462b444` cursor/auto RUN 0.66 II "Blind-lane candidate cut" 24m 51s / — / subscr / 136M ·
  `t-85e75668` grok/grok-4.6-build DONE IV "Fill-rate regression pass" 45m 59s / 8.8M / $1.37 ·
  `t-576eeb2d` gemini/gemini-3.1-pro FAIL 0.31 III "Venue and pool discovery" 16m 06s ·
  `t-22c5346d` cursor/"Auto (asked)" DONE II "Ladder-gas study" 54m 08s / subscr ·
  `t-a05e22ce` opencode/auto FAIL 0.08 I "Blind lane locator" 12s ·
  `t-d5a7f26e` gemini/gemini-3.1-pro DONE II "Call-site rescue" 23m 59s ·
  `t-23c61dfd` cursor/"Auto (asked)" FAIL 0.55 IV "Blind-lane cost measurement" 30m 36s / subscr ·
  `t-9cda19fa` cursor/auto STOP 0.22 II "Measurement retry" — / subscr
- **SEC-02 `poolstra-decompounder`** (wg `a279c7ca`), all grok unless noted:
  `t-6690cdb4` grok-4.6 RUN 0.78 III "Joint audit of two accounting changes" 44s / 32M ·
  `t-d9e2333b` DONE II "Rounding correctness review" 18m 08s / 2.3M / $0.36 ·
  `t-59ff4757` DONE II "Re-audit after rounding fix" 19m 00s / 2.2M / $0.40 ·
  `t-fcafc202` DONE II "Re-audit after withdrawal fix" 18m 35s / 2.6M / $0.42 ·
  `t-f886029c` DONE III "Fee-split correctness review" 22m 04s / 2.8M / $0.50 ·
  `t-2bc437e2` DONE IV "Compounding interval measurement" 17m 16s / 3.6M / $0.46
- **SEC-03 `aid-core`** (wg `41ba0dd2`): `t-0c41aa19` codex/gpt-5.6 RUN 0.34 II
  "Batch progress plumbing" 6m 12s / 0.6M / 98M ·
  `t-77b2e004` opencode/glm-4.7 DONE I "Rename watch engine symbols" 3m 41s / 0.2M / $0.01 ·
  `t-b1930f5c` kilo/default DONE I "Type fixes in the store layer" 1m 58s / free ·
  `t-9f10c3d2` codex/gpt-5.6 DONE V "Cut release v8.78.0" 12m 27s / 1.9M / $0.31 ·
  `t-e5567a2b` cursor/composer-1.5 STOP 0.44 II "Dashboard table restyle" — / subscr

Payload examples: `t-85e75668` → REPORT "Fill-rate regression", legendary,
`9 venues` · `8.8M · $1.37`; `t-9f10c3d2` → RELEASE "aid v8.78.0", legendary,
`3.4 MB bin` · `32 commands`; `t-b1930f5c` → FIXTURE "Store type fixes", common.

## 8. Game layer

- **XP / rank**: start at 4280. A completed mission awards its payload rarity XP; a
  lost one awards 20. Rank = `["I"…"VIII"][min(7, xp / 1000)]`, bar shows
  `xp % 1000 / 1000`. XP is client-side and persisted locally — the server has no
  concept of it and must not grow one.
- **Toast**: slides in from the trailing edge — result mark, title, mission id,
  `+<n> XP`, auto-dismiss after 5s. Green edge for DONE, red for FAIL.
- **Sound** (respects the KLAXON mute; ambient session so it never stops music):
  DONE = 523, 698, 880, 1046 Hz, 85ms apart, 200ms decay; FAIL = 330, 220, 165 Hz.
  Triangle wave in starship, square wave in pixel.
- **Haptics**: iPad only — `.success` on DONE, `.error` on FAIL, `.selection` on tab
  and theme change. Mac has none; do not fake it.
- **Reduce Motion**: when the system setting is on (or overridden in Settings) every
  loop stops — no sweep, no scanline drift, no blink. The console must stay fully
  readable without animation; never encode information in motion alone.

## 9. Structure

```
client/
  project.yml                  # xcodegen — the .xcodeproj is generated, gitignored
  DESIGN.md                    # this file
  AIDCommand/
    App/                       # entry, window/scene setup, root shell, HUD, menus
    Theme/                     # ThemeTokens, StarshipTheme, PixelTheme, environment
    Primitives/                # every theme-aware view (§4.3)
    Screens/                   # FleetLog, Hangar, Cargo, MissionBrief, Settings
    Model/                     # Mission, Sector, Payload, Unit, FleetSnapshot, XP
    Data/                      # FleetDataSource, DemoSource, LiveSource, SSEClient, Keychain
    Support/                   # generated Info.plists
    Resources/
  AIDCommandTests/
```

Repo limits apply: file ≤ 300 lines, function ≤ 50 lines, a 2–4 line header comment
on every file saying what it is and what it exports, strict typing, no force unwrap,
no `try!`.

## 10. Build & verify

```bash
cd client && xcodegen generate && cd ..
xcodebuild -project client/AIDCommand.xcodeproj -scheme 'AIDCommand macOS' \
  -destination 'platform=macOS' -derivedDataPath /tmp/aid-client-dd build
xcodebuild -project client/AIDCommand.xcodeproj -scheme 'AIDCommand iPad' \
  -destination 'generic/platform=iOS Simulator' -derivedDataPath /tmp/aid-client-dd build
```

**Both schemes must build.** A change that only compiles on Mac is not done.

Tests cover the pure logic: status mapping, rarity/XP math, progress derivation, the
demo tick, layout-class selection, and decoding a real captured `/api/fleet` payload.
Do not write snapshot tests for the themes; assert the token contract instead —
every theme supplies every token, and the two differ on panel shape, texture and
accent.

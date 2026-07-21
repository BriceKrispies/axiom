# Arena Forge — Architecture

Arena Forge is an eight-player deterministic auto-battler. This document
records its architectural placement under Axiom's laws, the ownership map of
every concern in the codebase, the exact simulation→presentation data flow,
and the determinism guarantees the implementation provides.

## Placement

Arena Forge is an **App** (`apps/arena-forge/web/`), in the same sibling
category as the other pure-TypeScript Axiom apps (`apps/axiom-heat-check`,
`apps/axiom-three-point`, `apps/casino-games`). It is **not** a Rust layer or
module: it has no `Cargo.toml`, no `layer.toml`, no `module.toml`, no
`app.toml`, and it does not appear in `cargo metadata` or the Rust workspace's
`members` list. `cargo xtask check-architecture` — which classifies every
*Cargo* package into Layer/Module/App/Tool and enforces the Layer Law and
Module Law — has no opinion on it, because it is outside the Cargo graph
entirely. The governing discipline for this app is the TypeScript-spine style
established by `@axiom/client` and `@axiom/web-engine` (tsgo, branchless
non-test code, 100% coverage on the reusable parts), not the Rust Module Law.

Why no new layer or module: nothing here is a reusable engine capability that
other apps need. The auto-battler's phase machine, economy, effect language,
and combat resolver are specific to Arena Forge's own game rules — exactly
the shape of a **leaf composition root** (an app), not a layer (broad,
shallow, reused by many apps) or a module (an isolated engine capability like
scene/render/assets). The one genuinely reusable dependency Arena Forge has is
`@axiom/web-engine` — declared via the `@axiom/web-engine` path mapping in
`web/tsconfig.json` and consumed the same way `axiom-heat-check` and
`axiom-three-point` do — but **only for two engine services**: the
fixed-step loop (`startLoop`, consumed by `src/harness.ts`) and procedural
audio (`playTone`, consumed by `src/audio/cues.ts`). Arena Forge does not
consume `@axiom/web-engine`'s renderer at all. The 2D presentation —
everything drawn on screen — is **app-owned Canvas2D** (`src/ui/draw.ts`,
`src/ui/render.ts`), which is the required baseline for this game: no
WebGL/WebGPU is needed to play it, and none is used. `src/sim/isolation
.test.ts` mechanically enforces the DOM-free/renderer-free half of this split
by scanning every non-test file under `src/sim/` for forbidden tokens
(`@axiom/web-engine`, `document.`, `window.`, `requestAnimationFrame`,
`HTMLCanvas`, `CanvasRenderingContext`, `getElementById`) and failing if any
appear — the simulation cannot even accidentally couple to the engine or the
browser.

**The simulation is DOM-free and renderer-free by construction.** Every file
under `src/sim/` is plain TypeScript: data interfaces, pure functions, and one
small `Match` class holding only plain data + a `Rng` + an event log. Nothing
in `src/sim/`, `src/api/`, `src/bots/`, or `src/harness/` imports `@axiom/
web-engine`, `web_sys`-equivalent browser globals, or any DOM type — this is
verified structurally (`web/tsconfig.json` excludes `*.test.ts` because the
tests run under bare `node --test`, and none of the sim/api/bots/harness
source imports the engine package). This is what lets the whole simulation run
headless under Node with zero browser shimming (see `harness/headless.ts` and
`TESTING.md`).

## Current implementation state

The app is complete end to end: the simulation package (`src/sim/`), the
transport-neutral API (`src/api/`), the bots (`src/bots/`), the headless
harness (`src/harness/`), and the browser-facing shell (`src/presentation/`,
`src/audio/`, `src/ui/`, `src/game.ts`, `src/harness.ts`, `index.html`) all
exist and are tested. `node --test "src/**/*.test.ts"` type-strips and runs
the sim/api/bots/harness test suite headlessly; `tsgo -p tsconfig.json
--noEmit` type-checks the whole app (including the UI) clean under `strict`;
and the real app has been **browser-verified**: `web/browser
/interaction_test.py` (Playwright) drives it at three landscape viewports —
844×390 (mobile), 932×430 (larger mobile), 1440×900 (desktop) — through a
full interaction path (inspect a shop card, buy it, play/reorder, reroll,
freeze, expire the shop timer via a deterministic test control, observe
combat, reach the next shop), asserting authoritative state at each step
through the game's dev handle (`window.__arena`), and captures the four
required screenshots (shop, combat, forged unit, final results). There are no
reserved/empty directories left in the app.

## Ownership map

| Area | Owning path(s) |
|---|---|
| App bootstrap / Axiom integration | `index.html` (the single-canvas page — one `<canvas id="arena-canvas">`, all UI drawn on it), `src/harness.ts` (the browser boot / platform edge: DPR-aware canvas sizing + resize handling, pointer wiring (one touch+mouse path via Pointer Events), and driving `@axiom/web-engine`'s `startLoop`  — the ONLY file that touches the DOM/wall clock), `src/game.ts` (`ArenaForgeGame` — orchestrates the host + interaction + audio + renderer, and paces combat playback) |
| Sim model (state shapes) | `src/sim/model.ts` (`MatchState`, `PlayerState`, `UnitInstance`, `ShopSlot`, `Pairing`, `PoolState`, `WarbandSnapshot`), `src/sim/ids.ts` (id vocabulary + `InstanceIdAllocator`), `src/sim/tuning.ts` (`Rules` / `DEFAULT_RULES`) |
| Commands + validation | `src/sim/commands.ts` (the `Command` union + `REJECT` reasons), `src/sim/economy.ts` (`applyShopCommand` — the sole validator/applier) |
| Phase machine | `src/sim/phase.ts` (`LEGAL_TRANSITIONS` / `isLegalTransition` — the independent spec), orchestrated by `src/sim/match.ts` (`Match.advancePhase`/`tick`) |
| Economy + shop | `src/sim/economy.ts` (buy/sell/reroll/freeze/upgrade/play/return/reorder), `src/sim/effects/economy-effects.ts` (economy trigger interpreter + `grantRoundGold`) |
| Shared pool | `src/sim/pool.ts` (`initPool`, `drawFromPool`, `rollShop`, `returnToPool`/`returnShop`, conservation) |
| Forging | `src/sim/forge.ts` (`resolveForges`/`forgeOnce` — data-driven, no card-specific code) |
| Combat | `src/sim/combat/board.ts` (live battlefield), `src/sim/combat/engine.ts` (`runCombat` — the deterministic loop), `src/sim/combat/env.ts` (bounds/counters), `src/sim/combat/combat-effects.ts` (combat trigger interpreter) |
| Pairing + standings | `src/sim/pairing.ts` (reseeding, rematch avoidance, ghost selection), `src/sim/resolution.ts` (`applyRoundResolution` — damage, elimination, placement tiebreak) |
| Effect language + interpreter | `src/sim/effects/language.ts` (the typed verb/trigger/condition/selector vocabulary + `EFFECT_BOUNDS`), interpreted by `src/sim/effects/economy-effects.ts` (economy context) and `src/sim/combat/combat-effects.ts` (combat context) |
| Content schema + validation | `src/sim/content/schema.ts` (`CardDefinition`/`GroupDefinition`/`VisualProfile`/`ContentBundle`), `src/sim/content/validate.ts` (`validateContent`), `src/sim/content/load.ts` (`LoadedContent` — canonicalization + indexing) |
| Content data | `src/sim/content/archetypes.ts`, `keywords.ts`, `groups.ts`, `visual-profiles.ts`, `cards/{ironbound,emberkin,bloomtide,echowisp,neutral,tokens}.ts`, assembled by `src/sim/content/bundle.ts` (`CONTENT`, `loadDefaultContent`) |
| Bots | `src/bots/policy.ts` (contract + shared reasoning helpers), `src/bots/policies.ts` (the three named policies), `src/bots/driver.ts` (`runBotTurn`) |
| Replay + serialization | `src/harness/serialize.ts` (`MatchReplay`, `serializeReplay`, `matchFingerprint` — the versioned snapshot/replay record: content/rules versions + seed + initial players + the full command log + the phase-transition log), `src/harness/replay.ts` (`replayMatch` — rebuilds a `Match` from a `MatchReplay` by pure input substitution, replaying the same seed and logged commands to a byte-identical final state and event log). `MatchState` (`sim/model.ts`) itself remains plain, structurally-clonable data underneath this. |
| Presentation-event adapter | `src/presentation/combat-playback.ts` (`reconstructFrame` — reconstructs a `CombatFrame` — unit health/slots/liveness, the active attacker/defender, damage floaters — purely by replaying a combat's `SimEvent` stream forward from its two immutable `WarbandSnapshot`s up to a time cursor; the renderer never decides anything, it only shows what the events already describe, and fast-forwarding the cursor never changes results), `src/ui/theme.ts` (`PALETTE`, `STAGE_TREATMENT` — the presentation-only palette and the per-`ArenaStage` visual treatment, derived one-way from the sim's `ArenaStage`; the simulation never sees a color) |
| Mobile interaction | `src/ui/interaction.ts` (`Interaction` — the pointer controller: tap to inspect, tap the contextual action to buy/play/sell, drag shop→hand/warband to buy-and-place, drag hand→warband to play, drag warband→warband to reorder, drag→sell zone to sell, tap reroll/freeze/upgrade, tap-hold for the full rules view; every gesture resolves to an authoritative `Command` submitted through the match API, never a direct state mutation — a cancelled/invalid drag submits nothing) |
| Rendering | `src/ui/render.ts` (`renderFrame` — a pure function of match view + layout + ui state + combat playback frame that paints the whole frame; Canvas2D only, no WebGL), `src/ui/draw.ts` (the immediate-mode Canvas2D primitive toolkit — rects, panels, text, hit-testing — the only surface that touches pixels), `src/ui/layout.ts` (`computeLayout`/`inspectRects` — the responsive, mobile-first layout solver; landscape-first from an 844×390 baseline, every touch target ≥ 44 CSS px) |
| Audio | `src/audio/cues.ts` (`AudioCues` — maps `SimEvent.kind` to a procedural tone via `@axiom/web-engine`'s `playTone`, filtered to the human player's own events and throttled to a max-tones-per-drain budget; muting or dropping cues cannot change a result, since audio only reads the event stream) |
| Headless harness | `src/harness/headless.ts` (`runSeededMatch`, `runManyMatches`, `runDefaultSuite`) |
| Tests | Co-located `*.test.ts` next to source (native `node --test` type-stripping, no build step) across `src/sim/`, `src/harness/`, and `src/sim/content/` — including `src/sim/isolation.test.ts` (the structural DOM/renderer-free guard for `src/sim/`) and `src/harness/headless.test.ts` (determinism + 100-match suite invariants) — plus the browser-level test: `web/browser/interaction_test.py` (Playwright; see "Current implementation state" above). |

## The simulation → presentation data flow

```text
Command (sim/commands.ts)
  → MatchApi.submit(CommandEnvelope)          (api/match-api.ts, api/envelopes.ts)
  → LocalMatchHost.submit → Match.submit       (api/local-host.ts, sim/match.ts)
      — the ONLY writer of MatchState; validates fully before any mutation
  → SimEvent stream                            (sim/events.ts — EventSink)
  → presentation adapter (combat-playback)      (presentation/combat-playback.ts — reconstructFrame)
      + audio                                    (audio/cues.ts — AudioCues.play)
  → Canvas2D renderer                           (ui/render.ts + ui/draw.ts, orchestrated by game.ts)
```

`src/game.ts`'s `ArenaForgeGame` is the orchestrator that wires this whole
chain together every fixed tick: the human's pointer gestures
(`ui/interaction.ts`) submit `Command`s through `host.submit` — **exactly the
same call a bot makes** (see `MULTIPLAYER_PROTOCOL.md`); `update()` drains new
events via `host.eventsSince(cursor)` and forwards them to `AudioCues.play`;
and on entering the `combat` phase it locates the human's `RoundResult`, pulls
that combat's own event slice (filtered by `combatId`), and paces
`combatCursor` forward across the phase's real-time window, handing the
current cursor to `reconstructFrame` every `render()` call.
**The renderer and the audio layer never decide a result — both only ever
read the already-decided `SimEvent` stream and `MatchState` snapshot.**

Every actor — the human UI (`src/game.ts` + `src/ui/interaction.ts`), a bot,
or a future remote client — talks to the match **only** through `MatchApi`: `submit`, `view`,
`eventsSince`, `isComplete` (`api/match-api.ts`). There is no second path into
`MatchState`. `Match` (`sim/match.ts`) is the sole authoritative state holder:
it owns the `MatchState`, the match's one `Rng`, the `InstanceIdAllocator`,
the `EventSink`, the `GhostStore`, and the replayable command log.

**Combat is computed once, then played back.** The `combat_prepare` phase
(`Match.enterCombat`) is where all of a round's combat actually happens: it
snapshots every active player's warband (`snapshotWarband`, `combat/board.ts`
— an immutable deep copy), computes pairings (`pairing.ts`), and for every
pairing runs `runCombat` (`combat/engine.ts`) to completion **synchronously**,
emitting the full ordered event stream for that fight into the same
`EventSink` the rest of the match uses. Only after every pairing's combat has
fully resolved does the phase advance to `combat`. The `combat` phase itself
performs **no simulation** — it is purely the time window
(`combatPlaybackSeconds` × `FIXED_HZ` ticks) during which the presentation
layer (`src/game.ts` + `src/presentation/combat-playback.ts`) reads the
combat's event slice and animates the already-decided event stream by
advancing a time cursor through it (see "The simulation → presentation data
flow" above). **The renderer never decides a combat result; it only replays
one.** This is
also why "replay combat from its event stream" is exactly `EventSink
.combatStream(combatId)` — a filter over the same log, not a special code
path.

## Determinism guarantees

- **One seed, one `Rng` class.** `sim/rng.ts` implements mulberry32 with
  rejection-sampled `range()` (no modulo bias). The match seed is folded
  through `deriveSeed(...)` (a splitmix32-style avalanche) with an explicit
  named context for every independent sub-stream: the match's own `Rng` is
  seeded `deriveSeed(seed, 0x4152454e)` (`match.ts`), and each combat gets its
  own seed `deriveSeed(state.seed, round, combatId)` (`match.ts`
  `enterCombat`) so unrelated systems never share a draw sequence.
- **No `Math.random`, no wall clock, anywhere in `src/sim/`.** Shop
  timers/combat playback windows are expressed in fixed ticks
  (`FIXED_HZ = 30`, `tuning.ts`) advanced by an explicit `Match.tick()` call,
  never `Date.now()` or `performance.now()`.
- **Integer math throughout.** Every gameplay quantity (gold, attack, health,
  damage, costs) is an integer; `Rng` only ever returns integers.
- **Stable iteration order everywhere.** Pool rolls always iterate
  `LoadedContent.collectibleOfTier` in canonical `(tier, id)` order (never
  `Object.keys` order) weighted by remaining pool counts (`pool.ts`). Combat
  reads both sides in slot order (`combat/board.ts`). Content collections are
  sorted by id at load time (`content/load.ts`).
- **Plain, structurally-clonable state.** `MatchState` (`model.ts`) has no
  functions, classes, `Map`s, or `Set`s in its serializable shape — it
  round-trips through `JSON.stringify`/`parse` byte-for-byte.
- **Proven, not just claimed.** `harness/headless.test.ts` asserts that two
  `LocalMatchHost`s constructed from the same seed produce byte-identical
  final state **and** byte-identical event logs after running to completion
  (`JSON.stringify({ state, events })` equality), and that 100 seeded matches
  (`baseSeed=1..100`) all reach `match_complete` with zero illegal phase
  transitions and zero negative gold.
- **Bounded by construction.** Every unbounded shape in the effect language
  (repeat counts, summon counts, nesting depth, abilities-per-card,
  operations-per-ability) is capped at content-load time
  (`content/validate.ts`), and every *runtime* unbounded shape (events per
  combat, summons per combat, copied abilities per combat, attack actions per
  combat) is capped by `CombatCounters` (`combat/env.ts`) — hitting a runtime
  cap force-ends the combat as a diagnostic draw rather than hanging. See
  `EFFECT_LANGUAGE.md` for the exact bounds.

# Arena Forge — Agent Routing Guide

Read this first when working anywhere under `apps/arena-forge/`. It tells you where
things live, the invariants you must not break, the non-obvious gotchas that will
bite you, and the exact build/test commands. The repo-wide engine laws live in the
root `C:\dev\axiom\CLAUDE.md`; **this app is exempt from most of them** (see below).

## What this is

An original, **mobile-first auto-battler** (8-player lobby: 1 human + 7 deterministic
bots) plus a procedural **3D miniature system** and a developer **Figure Lab**. It is a
**pure-TypeScript app** under `apps/arena-forge/web/` — NOT a Rust crate. It has no
`Cargo.toml`, no `app.toml`, and is **invisible to the Rust workspace** (the Layer /
Module / App laws, `cargo test --workspace`, coverage, dylint, and `check-architecture`
do not see it). It consumes the engine `@axiom/web-engine` (pure-TS, no wasm) ONLY for
engine services: the fixed-step loop (`startLoop`), the 3D scene renderer
(`initRenderer`/scene store), and procedural audio (`playTone`). Everything else —
rules, content, bots, UI, figures, screens — is app-owned.

## The one rule that overrides everything

**The simulation (`src/sim/`) is authoritative and deterministic. Presentation
(figures, animation, particles, materials, cameras, UI) is downstream and MUST NEVER
affect match state** — not attack results, targeting, damage, death, summon placement,
timing, or outcome. Same `seed` + command stream ⇒ byte-identical state + event log
(proven by tests). `src/sim/` is **DOM-free and renderer-free** (`src/sim/isolation.test.ts`
enforces it) — never import the DOM, the engine, or `ui/` from `sim/`.

## Directory routing (dependency direction: top depends on nothing below it in sim)

| Area | Path | What lives here |
|---|---|---|
| **Simulation** (authoritative, deterministic, DOM-free) | `src/sim/` | `model.ts` (state), `phase.ts` (state machine), `economy.ts`/`pool.ts`/`forge.ts`/`tuning.ts`, `combat/` (board/engine/effects), `effects/` (declarative DSL), `content/` (schema/validate/load + cards/groups/keywords/tokens), **`events.ts` = the `SimEvent` stream, the ONLY presentation boundary**, `rng.ts` (seeded integer RNG — never `Math.random`), `match.ts` (orchestrator). |
| Transport-neutral API | `src/api/` | `match-api.ts` (interface), `local-host.ts` (`LocalMatchHost`: 1 human + 7 bots), `envelopes.ts`. UI/bots submit commands through this — never mutate state directly. |
| Bots | `src/bots/` | 3 deterministic policies (economy/synergy/tempo) via the public command surface. |
| Headless harness | `src/harness/` | `headless.ts` (100-match runner + invariants), `serialize.ts`/`replay.ts` (versioned replay). |
| Combat playback | `src/presentation/combat-playback.ts` | Pure REPLAY of the `SimEvent` stream → `CombatFrame`/`PlayUnit`. Decides nothing. |
| **Procedural 3D figures** | `src/figures/` | `grammar.ts` (data types), `meshgen.ts` (SDK-free geometry), `generator.ts` (`expandFigure`), `compose.ts` (flat hierarchy → WORLD transforms + `PoseDelta`), `variation.ts` (seeded), `bodyplans.ts` (per-group grammar), `registry.ts` (`figureForCard`, memoized), `languages/` (5 `GroupVisualLanguage`s), `scene/` (materials, `FigureInstance`, arena scene), `runtime/figure-director.ts`. |
| **App screens** | `src/screens/` | `screen.ts` (Screen contract + `resetEngineScene`), `router.ts` (`ScreenRouter`), `main-menu.ts`, `gameplay.ts` (the match orchestration), `figure-lab/` (`figure-lab.ts` = the all-figures gallery screen, `catalog.ts` = the pure filter/search/sort query, `gallery-layout.ts` = the pure grid geometry). |
| App shell + boot | `src/game.ts`, `src/harness.ts`, `index.html` | `game.ts` = `ArenaForgeGame` (the screen router, `window.__arena`). `harness.ts` = browser edge (two canvases, `startLoop`, pointer). |
| 2D overlay UI | `src/ui/` | `draw.ts` (primitives: `panel/button/text/rivet/inRect/Rect`), `layout.ts`, `interaction.ts` (gameplay pointer→commands), `render.ts` (`renderFrame`), `theme.ts` (`PALETTE`, `STAGE_TREATMENT`). |
| Audio | `src/audio/cues.ts` | `SimEvent` → `playTone`, throttled. |

## Mental models you need

- **Two stacked canvases.** `#arena-scene` (base) is the engine's 3D scene (`initRenderer`);
  `#arena-canvas` (overlay, transparent) is the 2D UI. Each frame: `renderScene3D()` (3D)
  then `render(ctx)` (2D overlay). **Canvas2D backend is the deterministic baseline**;
  `?backend=webgl2` is an optional enhancement. When 3D is active the 2D overlay
  `clearRect`s over the arena so the scene shows through.
- **Screen state is centralized** in `ScreenRouter` (`main_menu` / `gameplay` / `figure_lab`).
  Screens never navigate directly — they call `ScreenNav.goto`. Each `enter()` rebuilds the
  scene; `exit()` releases it. The app boots on `main_menu`.
- **Figure grammar.** A figure is a flat, parent-before-child part list; `composeWorld`
  flattens it to WORLD transforms on the CPU (the engine has **no parenting**). Animation =
  per-part `PoseDelta`s fed to `composeWorld`. Group coherence comes from `languages/` +
  `bodyplans.ts`; per-card variation from a stable `figureSeed(cardId, salt)`.
- **The event stream is the seam.** Presentation reads `SimEvent`s and immutable snapshots.
  Adding presentation must not require new sim decisions.

## Gotchas that WILL bite you

- **`node --test` uses Node's strip-only TypeScript**: NO parameter properties
  (`constructor(private x)`), NO `enum`, NO `namespace`, NO decorators in any file it loads
  (transitively via **value** imports; `import type` is erased). Use explicit field
  declarations + assignment. `tsgo` (which builds `dist/`) accepts all of it, so a file can
  typecheck yet fail under `node --test`. Keep test-reachable files strip-safe.
- **`@axiom/web-engine` scene store is a module singleton** with: NO per-node despawn (only
  `clearScene()`), NO transform parenting, NO viewport/offscreen/render-target, one
  full-canvas pass. Consequences: switching screens or changing a figure ⇒ `clearScene()` +
  `resetMeshCache()` + `resetMaterialCache()` + rebuild (use `resetEngineScene()`). You cannot
  render N independent mini-scenes — so the Lab gallery renders every figure at once in ONE
  scene under ONE camera: a fixed near-orthographic camera looking down −Z at the z = 0 plane,
  with `PX_PER_UNIT` making the screen→world mapping exact, so each grid cell's screen rect
  places its miniature and the 2D captions land on the right icons. Off-screen figures are
  `park()`ed (never despawned); only the FORGED toggle respawns.
- **Spin the gallery figures about Y, never X.** Every miniature shares one spin phase, so an
  X (tumble) rotation makes the WHOLE grid go edge-on and read as blank at the same moment.
  A Y turntable keeps every silhouette readable at every angle. (This looked like a
  "figures don't render on phone" bug; it was the edge-on phase.)
- **Materials**: `baseColor` + `emissive` + `opacity` only. No metallic; `roughness` is
  ignored. Transparency via `opacity < 1` (works on both backends).
- **A figure's ROOT part must not carry a rest rotation** — it rotates the whole body
  underground. World facing is applied by the `RootFrame` at pose time. Guarded by
  `figures/grounding.test.ts` (this is exactly the bug that hid the Emberkin tribe).
- **Sim balance invariants**: the consequence formula includes an anti-stalemate escalation
  term (without it, 2–3 static boards draw forever and the match never ends); a forged unit
  returns `copiesToForge` (3) copies to the pool on sell/elimination; an eliminated player's
  board is cleared after returning its cards. Don't "simplify" these away — determinism +
  pool-conservation + termination tests will fail.
- **Never `cargo fmt` spine crates**, and don't touch Rust — this app is TS-only.

## Build · serve · test · verify (from `apps/arena-forge/web/`)

```sh
# Serve (build + hot-reload at http://localhost:8080), from repo root:
cargo run -p axiom-serve -- arena-forge          # or: make dev arena-forge   (--no-open to suppress browser)

# Unit / integration / determinism / figure / screen tests (Node 24 native TS):
node --test "src/**/*.test.ts"

# Typecheck (vendored native TS compiler; excludes *.test.ts):
../../../packages/axiom-web-engine/node_modules/@typescript/native-preview-win32-x64/lib/tsgo.exe --noEmit -p tsconfig.json

# Browser interaction test (Playwright, 844×390 / 932×430 / 1440×900), from repo root:
uv run apps/arena-forge/web/browser/interaction_test.py
```

**Dev/test hooks** live on `window.__arena` (set in `harness.ts`): `debugGoto(screen)`,
`debugScreen()`, `debugAdvancePhase()` (expire the shop timer deterministically),
`debugLayout()`, `debugShowcaseForge()`, `debugSummary()`, `debugFigures()`,
`debugLabSelect(group, cardId)`, `debugLabForged(bool)`, `debugLabSearch(term)`,
`debugLabSort(mode)`, `debugLabZoom(factor)`, `debugLabInfo()`. Pin
`?backend=canvas2d` for byte-stable captures. Browser tests drive these — don't wait on
wall-clock.

## Current state (what's real vs. not yet built)

**Working + tested + browser-verified:** the full deterministic sim (38-card content, effect
DSL, 100-match harness, replay); 2D-UI gameplay; procedural 3D figures for every card
(rendering in shop/warband/combat); the screen-state shell; the main menu (real forge scene +
group reps); and the **Figure Lab** — a live gallery that renders EVERY card's real figure at once in one
scene, sectioned by tribe, each on a slow Y turntable, with search (`/` focuses; keyboard runs
through `Screen.onKey`), five sort modes, the group chips, resizeable icons (slider / wheel /
pinch / +-), a spin pause, and the inspector (real procedural inputs + group-language summary,
forged toggle). ~91 `node --test` tests pass; tsgo clean.

**Not yet built (`figures/anim/` and `figures/modifiers/` are EMPTY dirs):**
- A shared **animation state machine** (spawn/idle/attack/damage/death/victory/forge). Today
  figures only idle-bob + a combat root-frame lunge; `compose.ts`'s `PoseDelta` plumbing is
  ready but unused.
- A shared **visual-modifier system** + a **`ward` keyword** (only `guard`/`armored` exist in
  `sim/content/keywords.ts`). `grammar.ts` defines `FigureModifierVisual` types that nothing
  consumes yet.
- A figure **`validate.ts`** structural-validation module, a **telemetry/budgets** module, and
  a **capture/filmstrip** harness.
- Figure Lab **breadth**: animation controls, modifier/Divine-Shield controls, comparison mode,
  silhouette lineup, game-context + mobile-size previews, debug views, full validation panel.

Any of the above must be built as **shared** systems used by BOTH gameplay and the Lab — never
a Lab-only or duplicate implementation.

## Docs

`README.md` (overview + run), `ARCHITECTURE.md` (ownership + data flow), `GAME_RULES.md`,
`EFFECT_LANGUAGE.md`, `CONTENT_AUTHORING.md`, `MULTIPLAYER_PROTOCOL.md`, `MOBILE_UI.md`,
`TESTING.md`. Note: these predate the latest figure/screen work in some places — trust the
code + this file when they disagree, and update the docs as you go.

## Conventions

Every file opens with a comment stating its role and the invariants it upholds — match that
style. Keep `sim/` free of DOM/engine/UI imports. Prefer reusing the existing figure/UI/content
systems over adding parallel ones. When you touch presentation, prove the sim is unchanged
(re-run the determinism + harness tests).

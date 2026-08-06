# Casino Games

A reusable, data-driven engine for bright, cheerful prize games of chance —
one coherent chance framework, twenty games built as mechanic + presentation
adapters over it. Pure TypeScript on the Axiom TypeScript SDK
(`@axiom/web-engine`): no wasm, no Rust, no external assets. Rewards are
toys, tickets, stars, gems, and capsules — never money.

## Running it

```sh
# Dev server with hot reload (builds, serves web/, rebuilds on save):
cargo run -p axiom-serve -- casino-games            # http://localhost:8080/

# Build only:
npm --prefix packages/axiom-game exec -- tsgo -p apps/casino-games/web/tsconfig.json

# Tests (Node's runner, native TS type-stripping, no DOM):
node --test "apps/casino-games/web/src/**/*.test.ts"
```

`web/package.json` exists so bare `node --test` can resolve the
`@axiom/web-engine` specifier (a `file:` link into `packages/axiom-web-engine`);
run `npm install` inside `apps/casino-games/web/` once if `node_modules` is
missing. In the browser the same specifier is resolved by the import map
axiom-serve injects.

## Capture agent (screenshots of the real running app)

`web/browser/agent_capture.py` drives the served app in a headless browser the
way a player would — open a machine, wait for a phase, move the cursor, press an
action — and captures the frame. Control goes through the app's own affordances:
the boot URL below and `window.__casino` (the shell's capture/dev handle:
`games()`, `play(id, seed?)`, `back()`, `hud()`, `press(code)`,
`pointer(x, y, down)` in logical 960×600 canvas space). It is the browser-side
analogue of a native agent driver — this app is invisible to the Rust
`axiom-agent`.

```sh
uv run scripts/localhost_servers.py start-app casino-games --port 8087
uv run apps/casino-games/web/browser/agent_capture.py --scene chests-ready
uv run apps/casino-games/web/browser/agent_capture.py \
    --do play:treasure-chest-pick phase:ready move:480,300 shot
```

Verbs: `play:<gameId>[@seed]`, `back`, `phase:<name>`, `wait:<ms>`, `key:<code>`,
`move:<x,y>`, `click:<x,y>`, `shot[:name]`. Prefer `phase:` over `wait:` so a
capture never races the fixed-step loop. Pair `--shot N` (freeze) with `--seed N`
for a byte-stable frame; `--clip native` writes the canvas backing store exactly
(960×600). The visual-convergence champions under `visual_targets/` are captured
this way — see `visual_targets/treasure-chest-pick/capture.md`.

URL affordances: `?game=<id>` boots straight into a game, `?seed=N` pins the
session seed, `?shot=N` freezes the simulation at tick N (deterministic
screenshots; also pins the wall clock), `?press=Code@tick,...` scripts key
presses (e.g. `?press=Enter@140`), `?backend=canvas2d|webgl2` forces a render
backend, `?debug=1` opens the diagnostics drawer, `?workbench=1` opens the
workbench for `?game`.

## Headless frame-rate regression test (Canvas2D, no browser)

`games/treasure-chest-pick/frame-rate.test.ts` plays a whole chest round under
bare `node --test` — boot, intro, **click** the centre chest, then the crab's
errand, the spiral into close-up, the latch, the lid, the treasure and the
celebration — and times **every frame of it** on the engine's real Canvas2D
software rasterizer.

```sh
node --test "apps/casino-games/web/src/games/treasure-chest-pick/frame-rate.test.ts"
```

It prints a per-phase report (`OVERALL … fps`, then `intro` / `ready` /
`committing` / `revealing` / `celebrating`) and then gates on it. At the shipped
default quality (936×585 CSS, `renderScale` 0.5 → 137k samples/frame) it
reproduces the numbers `treasure-chest-pick/definition.ts` documents.

Everything below the harness is the shipped path: `TREASURE_CHEST_PICK.mount` →
`mountCasinoGame` → `runGame` → `initRenderer(…, "canvas2d", …)` → the real
z-buffered scanline rasterizer writing a real framebuffer. The browser surface it
needs comes from `games/headless-canvas2d.testkit.ts`, which is deliberately
strict: it throws the moment the engine touches a DOM member it does not
implement, because a permissive stub would silently stop measuring work. Two
costs are counted rather than paid, and its header says so — the framebuffer
present (`putImageData`) and the water overlay's final vector fills (the
overlay's *geometry* runs for real).

The engine's clock is virtual and steps exactly one fixed tick per frame, so the
round is byte-identical on every machine (always the same 333 frames); the
measurement uses `process.hrtime` and is unrelated to it. The treasure is pinned
through the shipped `InjectedChanceResultSource`, so the work is the same every
run — one treasure is enough, the ritual is the same for all five.

**The scene-cost ceiling is the real regression guard.** Wall-clock fps on a dev
machine drifts by well over 50% run to run (`bench_rasterizer.py` documents the
same code measuring 20.6ms and 34.1ms minutes apart), so a before/after fps
comparison cannot see a regression worth less than ~2×. So the test also pins the
scene's *size*, which has no noise at all — this round costs exactly **398 peak
nodes/frame and 124,517 node-frames**, identical on every run, and the software
rasterizer costs ~1 unit of time per node per frame. The ceilings sit ~3% above
that, deliberately thin.

The asymmetry is the point, and it is what makes this safe to run a
`/visual-convergence` campaign against: a camera, grade, exposure, fog or
light-rig change moves node count by **exactly zero** and passes freely, while
added geometry trips it immediately. If a change needs that geometry for the
hardware renderer, gate it on the tier (`rendererTierAtLeast`) rather than raising
the ceiling — that is the fix the failure message asks for.

Alongside the frame rate it asserts the round really happened: the committed
outcome resolved against the chest the *click* landed on, the commit beat and the
reveal ritual each ran their full timeline, and every frame presented a
full-resolution framebuffer. The fps floor is a **ratchet** (like the Rust
render-churn gate) — raise it as the software path gets faster, never lower it to
make a change pass. Override it for a local run on a busy machine with
`AXIOM_CHEST_FPS_FLOOR=10`.

This is the complement to `web/browser/bench_render.py`, which times the renderer
alone on a frozen scene in a real browser. That one judges a rasterizer change;
this one is the number the player actually lives with, and it runs in CI-shaped
conditions with no browser and no served build.

## The canvas-free CSS 3D build

There are two ways to see Treasure Chest Pick rendered without a canvas, both
driven by the SAME chance engine — they differ only in how pixels are produced.

**1. `?backend=css` — the engine's DOM backend.** `@axiom/web-engine` has a
third `RenderBackend` (`backend-css.ts`) alongside WebGL2 and Canvas2D. It never
acquires a drawing context: it merges each mesh's coplanar triangles into convex
polygon faces (a box's 12 triangles become 6 quads), emits one absolutely-
positioned element per face mapped into 3D by `matrix3d`, and shades each face
with the shared `shading.ts` truth so colors match the other backends. The
`<canvas>` stays in the page as a transparent layout/pointer anchor and is never
drawn into. It works for ANY game in the catalog:

```sh
uv run scripts/localhost_servers.py start-app casino-games --port 8087
# then open http://localhost:8087/?game=treasure-chest-pick&backend=css
```

It holds **60fps** on the chest scene, via a screen-space LOD: back-facing faces
are dropped outright (exact, and ~half of every box), and nodes/faces below a
projected-size threshold are culled. That takes a scene authored for a GPU (359
nodes / ~3.6k faces, which composites at ~2fps) down to ~390 painted elements.
The LOD is continuous, so detail returns as things get closer — the chest's
nameplate lettering is below threshold on the board and reappears, spelling the
brand, once that chest flies to its hero reveal.

**2. `/css3d.html` — a build authored FOR the DOM.** The same game at 60fps, by
spending the element budget deliberately: 13 elements per chest instead of 246,
gradients instead of geometry for plank seams and lid curvature, and a lagoon
rebuilt from CSS gradients instead of the Canvas2D water overlay. ~230 elements
total, of which only the nine chest wrappers move per frame.

```sh
# http://localhost:8087/css3d.html   (?seed=N pins the round)
```

It is **not** a reimplementation. `src/chest-round/round.ts` imports the real
`planChoicePopulation`, the real config schema and validation gate, and the real
seeded streams, so a click resolves through exactly the code the engine build
runs. Layers:

```text
web/css3d.html            the 3D layer stack (see styles/css3d.css)
web/src/css3d/
  render/solid.ts         primitives: a solid as up to 4 CSS 3D planes
  scene/chest.ts          one chest (nested transform tree, hinged lid)
  scene/diorama.ts        sand, CSS lagoon, palm, sandcastle, crab
  main.ts                 shell: seed, DOM events, idle loop
web/src/chest-round/
  round.ts                the REAL chance engine, wired — shared by the CSS 3D
                          page, the resilient page, and the stand-in server
```

## The resilient (form-first) build

**`/resilient.html` — the same game as a plain HTML form.** Built for a
locked-down enterprise (Citrix) browser, where JavaScript may not run and
stylesheets may not survive. The BASELINE is a genuine
`<form method="POST" action="/api/pick">` with nine `<button type="submit">`
chests: press one, the browser navigates, and a server-rendered result page
comes back. Everything richer is layered on top and every layer is optional:

It carries the WHOLE capability ladder — the same one `@axiom/web-engine`
detects, with the document itself as its bottom rung:

| rung | drawn by | a pick is… |
|------|----------|------------|
| `webgpu` / `webgl2` / `webgl1` / `canvas2d` | the engine: the shipped **Treasure Chest Pick** on a canvas layered UNDER the buttons | posted in place; the server's answer is injected into the game, which flies the chest to its close-up and opens it |
| `css3d` | this page's CSS 3D chests, built INSIDE the buttons | posted in place, revealed without navigating |
| `form` | nobody — the served document | a native form navigation to an HTML result page |

The render rung is **not guessed**: `detectTier()` paints a known pattern on
each rung and classifies the pixels that come back, so a context that exists
but renders nothing is rejected. `?render=<rung>` forces any of them
(`?render=auto` clears the session pin). The page adds exactly one judgement of
its own — whether ITS stylesheet applied, read off a `--resilient-css` sentinel
— because a stripped stylesheet pins the page at `form`.

The POST is **identical at every rung** — the same endpoint, the same fields,
the same `resolvePick` on the server — so what the no-JS rung exercises is what
ships. There is exactly one `POST /api/pick` in the build: the engine-rendered
rung posts nothing, it is HANDED the answer. The page publishes the rung it
actually reached as `window.__axiomTier` (plus a `data-axiom-tier` attribute, a
`postMessage` to the parent, and the whole probe report on `window.__renderProbe`
for support diagnostics), and DOWNGRADES it — walking one rung down and
republishing — if an enhancement will not mount, or if the transport turns out
to be a lie: `fetch` can be present and still reject under `connect-src 'none'`,
so the fallback ladder is try/catch-based (fetch → `XMLHttpRequest` → let the
native form go), never `typeof`-based.

**The backbone never moves.** At every rung the nine
`<button type="submit" name="pick" value="N">` controls stay in the DOM, stay
enabled and stay the only interaction: the engine's canvas is inserted behind
them with `pointer-events: none` and the buttons are repositioned over the
projected chests, exactly as the CSS 3D chests are built inside them. The engine
game's keyboard actions are unbound at mount, so there is no second control that
could disagree with the form about which chest was chosen.

```sh
uv run scripts/localhost_servers.py start chest-resilient -- node tools/axiom-chest-server/src/main.ts --port 8090
# then open http://localhost:8090/
```

The server is `tools/axiom-chest-server` — one port for the static page and the
POST endpoint, because a native form navigation has no CORS story. See its
README.

```text
web/resilient.html        the `form` rung: a real form, usable with no CSS and no JS
web/styles/resilient.css  nine buttons decorated into chests, no extra DOM
web/src/resilient/
  contract.ts             the wire shape, shared with the server
  tier.ts                 the ladder's rules (pure) + the stylesheet sentinel
  transport.ts            fetch -> XHR -> give up, all inside try/catch
  outcome.ts              the words a result is reported in (pure)
  chests-3d.ts            css3d rung: css3d/scene/chest.ts dropped in the button
  board-layout.ts         where the buttons sit over the rendered chests (pure)
  injected-outcome.ts     the server's answer, as the chance engine wants it (pure)
  engine-board.ts         canvas2d+ rung: the shipped game, under the buttons
  main.ts                 shell: the ladder walk, submit interception, fallback
```

## Architecture

```text
web/src/
  chance-engine/        the game-agnostic chance framework (pure, node-tested)
    configuration/      versioned config schema, validation, JSON import/export
    randomness/         named deterministic streams (pure hash — no RNG state)
    probability/        the four mechanic adapters
    outcomes/           OutcomePlan + the result-source boundary
    sessions/           phase machine + session state + commitment rules
    registry/           CasinoGameDefinition + CasinoGameRegistry
    diagnostics/        the per-session audit record
  presentation/         shared stagecraft (cameras, glass, rewards,
                        celebrations, audio cues, props, easing, vectors)
  games/                round-state.ts (pure fold) + casino-mount.ts (engine
                        shell) + choice-input.ts + one directory per game
  application/          the DOM shell (screens, settings, config store)
  catalog/              catalog cards, filters, procedural thumbnails
  workbench/            the configuration workbench
  chest-round/          the chest board's rules over the chance engine, with no
                        presentation attached — shared by the two pages below
                        and by tools/axiom-chest-server
  css3d/                the canvas-free CSS 3D page
  resilient/            the form-first page, carrying the whole ladder
                        (form -> css3d -> canvas2d -> webgl1/2 -> webgpu)
  main.ts               boot
```

**Layering.** Every game is `definition.ts` (catalog metadata + default config
+ `mount`) over `game.ts` (a pure per-tick controller) over `scene.ts` (a pure
view returning an engine `Scene` value). All twenty games run through ONE
harness: `round-state.ts` owns the shared pure fold (phase mechanics, input
locking, commitment hand-off, resets) and `casino-mount.ts` runs that fold
inside `runGame`. Game modules never import the engine as a value — engine
shapes appear as types only — so every controller is testable under bare
`node --test`.

## The registry

`games/index.ts` is the single source of truth: it registers all twenty
definitions into a `CasinoGameRegistry`. The catalog renders from the
registry, the shell mounts through it, the workbench pulls defaults from it,
and `registry.test.ts` asserts the twenty required ids exist exactly once.
Registration rejects duplicate ids and default configs that fail validation.

### Adding a new game

1. Create `web/src/games/<id>/` with `definition.ts`, `game.ts`, `scene.ts`,
   and a focused `<id>.test.ts`.
2. Pick the mechanic adapter (`choice-population`, `destination`,
   `combination`, or `single-reveal`) and declare it as the mount spec's
   `mechanic`.
3. Author the controller (`step`) against the session phase machine and the
   view against the shared presentation systems.
4. Register the definition in `games/index.ts` (and its mechanic in
   `mechanicInitFor`). The registry tests then hold it to the same contract
   as the other games.

## Configuration schema

Every game runs from a versioned `CasinoGameConfig` (see
`chance-engine/configuration/schema.ts`):

```jsonc
{
  "schemaVersion": 1,
  "gameId": "treasure-chest-pick",
  "displayName": "Treasure Chest Pick",
  "targetWinRate": 0.42,            // total win probability, in [0, 1]
  "rewardTiers": [                   // weights are CONDITIONAL ON WINNING
    { "id": "common",  "label": "Star Token",     "rarity": "common",
      "weight": 60, "countsAsWin": true,
      "reward": { "kind": "stars",    "label": "25 stars",       "amount": 25 } },
    { "id": "uncommon","label": "Ticket Bundle",  "rarity": "uncommon",
      "weight": 28, "countsAsWin": true,
      "reward": { "kind": "tickets",  "label": "120 tickets",    "amount": 120 } },
    { "id": "rare",    "label": "Gem Trophy",     "rarity": "rare",
      "weight": 10, "countsAsWin": true,
      "reward": { "kind": "gems",     "label": "Radiant gem",    "amount": 1 } },
    { "id": "jackpot", "label": "Golden Capsule", "rarity": "jackpot",
      "weight": 2,  "countsAsWin": true,
      "reward": { "kind": "capsules", "label": "Golden capsule", "amount": 1 } }
  ],
  "choiceCount": 9,                  // choice games only
  "presentationSpeed": 1,            // 0.25..3 animation-duration multiplier
  "celebrationIntensity": 1,         // 0..2
  "cameraPreset": "tabletop",        // machine-interior | showcase | tabletop | reveal-focus
  "reducedMotion": "system",         // system | on | off
  "gameSpecific": { "danceLiveliness": 0.7 }
}
```

Validation (`validation.ts`) runs before any session may start:
`targetWinRate` must be finite in `[0, 1]`; tier weights finite and ≥ 0 with
at least one usable winning tier whenever wins are possible; unknown
`schemaVersion`s are rejected with a readable error, never coerced. Each
definition adds `validateSpec` for its `gameSpecific` block. The workbench
surfaces every issue verbatim and refuses to save or preview until clean.

**Target win-rate semantics.** `targetWinRate` is the authoritative total
probability that a round wins. Tier weights only distribute *which* tier a
win grants. Gameplay context (claw target, cast region) selects which visual
object or reward family manifests — never the win probability itself.

## The result-source boundary

The engine never calls `Math.random()` (a source-scan test enforces it).
Sessions resolve through exactly one `ChanceResultSource`:

- **`SeededChanceResultSource`** — dev, previews, tests, standalone play. The
  seed enters once at the outermost app boundary (`crypto.getRandomValues` in
  the shell, or `?seed=N`), is recorded immediately, and everything below is
  a pure function of it. Same seed + config + inputs ⇒ same outcome and the
  same significant animation decisions. "Replay Same Seed" recreates the same
  round; "New Round" advances the round counter under the same seed.
- **`InjectedChanceResultSource`** — integration with an authoritative
  service, app-local and transport-neutral (no server or protocol here).
  `supply(round, outcome)` delivers a committed outcome ({round id, win,
  tier, presentation seed, optional resolution data}); until it arrives the
  session simply stays uncommitted, and the game only animates and reveals
  what was supplied. See `session.test.ts` for the exact flow.

## Deterministic streams

All randomness is `sample01(seed, purpose, ...keys)` — a pure hash of the
seed, a named stream purpose, and integer keys. The purposes (`gameplay`,
`placement`, `tier`, `trajectory`, `ambient`, `particles`, `audio`, `camera`)
are the independence invariant: outcomes draw only from the first three;
everything decorative draws from the rest, keyed off the committed plan's
`presentationSeed`. Adding one extra sparkle can never change who wins —
`round-flow.test.ts` and the per-game tests pin this.

## Probability adapters

- **Choice population** (chests, cards, doors, presents, map, portals,
  rocks): for `n` objects at rate `p`, exactly
  `floor(n·p) + Bernoulli(frac(n·p))` objects win — assigned and placed (by
  the placement stream) BEFORE the player chooses; the pick only reveals its
  preassigned slot. Single-round realized probability is `winners/n`;
  repeated rounds converge to `p` (stochastic rounding, tested).
- **Destination** (drop, wheel, rocket, elevator, fountain, conveyor,
  lanterns): declared slots (tier or losing, with relative mass) compile so
  winning slots share exactly `p`; one draw commits the destination and the
  animation must arrive there plausibly — never a final-frame snap.
- **Combination** (dice, safe): the win state resolves at `p`, then a
  concrete winning combination (via tier weights) or a uniform losing one is
  committed; the dice/dials animate to exactly that combination.
- **Single reveal** (scratch, ball machine, fishing, claw): one Bernoulli
  commit at `p` + a conditional tier; player context picks the visual
  manifestation only.

## Fairness and commitment

The outcome commits at a clear point before its reveal (`commitOutcome`,
inside the "committing" phase): after that nothing can change it — the
session layer throws on a second commitment, the reveal phase is unreachable
without one, and input is hard-locked during committing/revealing/resetting.
Every completed session carries an audit record (game id, schema version,
config hash, seed/round id, commitment phase + tick, input context, result,
manifestation, completion tick, per-purpose stream seeds).

## Machine-camera rule

Games set inside a physical machine (Ball Machine, Claw Grab, Capsule
Conveyor) put the camera INSIDE the machine via the reusable
`machineInteriorCamera` preset: mounted near the upper-left interior corner,
aimed diagonally at the playable volume, slightly downward, stable during
interaction, with subtle cinematic movement only during the final reveal.
The housing (`machineHousing`) and the shared glass (`glassPane`: cyan tint,
edge highlights, two diagonal streaks — no refraction, no blur) frame the
view so the player feels enclosed.

## Diagnostics

Append `?debug=1` to show the development diagnostics drawer: session seed,
per-purpose stream seeds, committed outcome plan, phase, tick, choice
population / destination data, reward tier, and replay status. It is never
rendered in ordinary player mode, and the HUD never exposes an outcome before
its reveal (tested).

## Validation commands

```sh
npm --prefix packages/axiom-game exec -- tsgo -p apps/casino-games/web/tsconfig.json   # build
node --test "apps/casino-games/web/src/**/*.test.ts"                                    # app tests
node --test "apps/casino-games/web/src/games/treasure-chest-pick/frame-rate.test.ts"    # headless Canvas2D fps gate
cargo xtask check-architecture                                                          # repo architecture
cargo test --workspace                                                                  # Rust workspace
npm --prefix packages/axiom-web-engine run gate                                         # TS engine gate
cargo run -p axiom-serve -- casino-games                                                # run in browser
```

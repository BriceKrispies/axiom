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
cargo xtask check-architecture                                                          # repo architecture
cargo test --workspace                                                                  # Rust workspace
npm --prefix packages/axiom-web-engine run gate                                         # TS engine gate
cargo run -p axiom-serve -- casino-games                                                # run in browser
```

# Arena Forge

An original, mobile-first, single-canvas **auto-battler** built as an Axiom app.
Assemble a warband of magical constructs in a travelling forge arena, fight seven
deterministic bots across shop → combat rounds, forge duplicate units into
stronger versions, and survive to be the last competitor standing.

Arena Forge is a **pure-TypeScript app** under `apps/arena-forge/web/`. It is not
an Axiom layer or engine module and adds none — it composes the existing engine.
It consumes Axiom (`@axiom/web-engine`) only for engine services (the fixed-step
loop and procedural audio); everything else — rules, content, effect
interpretation, bots, match orchestration, UI, and presentation — lives inside
the app. The gameplay simulation (`src/sim/`) is DOM-free and renderer-free, so
the same code runs in the browser, a Node test runner, the replay tool, or a
future authoritative server.

## Play it

```sh
cargo run -p axiom-serve -- arena-forge          # build + serve at http://localhost:8080
# or:  make dev arena-forge
```

Then open the page on a phone (landscape) or desktop. One human plays against
seven bots; the match runs from lobby to the final surviving player.

- **Tap** a shop card to inspect it; tap **BUY** to purchase.
- **Drag** a shop card onto a warband slot to buy-and-place; drag a hand card
  onto a slot to play it; drag a unit between slots to reorder; drag a unit to
  the **SELL** zone to sell.
- Tap **REROLL**, **FREEZE**, **FORGE UP**.
- **Tap-hold** a card for its full rules; tap outside to close.
- Three normal copies of a unit **forge** automatically into one upgraded unit.

Mouse uses the exact same pointer path.

## What's here

| Area | Where |
|---|---|
| Deterministic simulation (model, phases, economy, forging, combat, pairing, effects) | `src/sim/` |
| Data-driven content — 4 groups × 8 units + 4 neutrals (36) + tokens | `src/sim/content/` |
| Transport-neutral authoritative API + in-process local host | `src/api/` |
| Deterministic bots (economy / synergy / tempo) | `src/bots/` |
| Headless harness (1 match, 100 matches, replay, invariants) | `src/harness/` |
| Presentation (combat playback from the event stream) + audio | `src/presentation/`, `src/audio/` |
| Single-canvas mobile UI (layout, draw, interaction, render) | `src/ui/` |
| App bootstrap | `src/game.ts`, `src/harness.ts`, `index.html` |

## Docs

- [ARCHITECTURE.md](ARCHITECTURE.md) — placement, ownership map, data flow, determinism.
- [GAME_RULES.md](GAME_RULES.md) — full rules: phases, economy, forging, combat, consequences, pairing, ghosts.
- [CONTENT_AUTHORING.md](CONTENT_AUTHORING.md) — add a group/unit/token/visual profile/forged ability without touching the engine.
- [EFFECT_LANGUAGE.md](EFFECT_LANGUAGE.md) — every trigger, condition, selector, operation, bound, and ordering rule.
- [MULTIPLAYER_PROTOCOL.md](MULTIPLAYER_PROTOCOL.md) — command/event envelopes, sequence + reconnect handling, remote-host seam.
- [MOBILE_UI.md](MOBILE_UI.md) — mobile-first layout, touch targets, gestures, the presentation boundary.
- [TESTING.md](TESTING.md) — how to run the tests, harness, and browser interaction test.

## Verify

```sh
cd apps/arena-forge/web
node --test "src/**/*.test.ts"                   # unit / integration / determinism / harness
# typecheck (vendored native TS compiler):
../../../packages/axiom-web-engine/node_modules/@typescript/native-preview-win32-x64/lib/tsgo.exe --noEmit -p tsconfig.json
# browser interaction test (serve first):
uv run apps/arena-forge/web/browser/interaction_test.py
```

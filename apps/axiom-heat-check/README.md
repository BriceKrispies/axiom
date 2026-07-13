# Heat Check

A stylized 3D basketball **scoring** game authored purely in TypeScript on the
`@axiom/game` SDK — not literal drag-a-ball-into-a-hoop, but the *emotion* of getting
hot: create space, rise up, release in rhythm, watch the arc, swish, build heat, keep
the streak alive.

You control a small procedural player on a half-court who auto-dribbles. A defender
mirrors you with a reaction delay and tries to stay between you and the hoop. **Drag
left/right (or A/D · ←/→) to create separation, then release (or SPACE) to shoot.** You
never touch the ball directly — you create space and release in rhythm; the shot's
success is decided by its *quality*, not by aiming a physics object.

Score as much as possible in **60 seconds**. Makes build **heat** (momentum); at high
heat the player glows, the shot trail brightens, the crowd/court pulses on a swish, and
the defender gets more aggressive — but shots are worth more. Misses cool you down and
break the streak. The final 10 seconds are **double points**.

## Scoring

- Normal make **2**, swish **3**, deep shot **+1**.
- Every 3 consecutive makes raises the multiplier by 1, capped at **4×**.
- Final 10 seconds double all points.
- Heat 0–5: makes warm you, swishes more, misses cool you (a bad miss resets momentum).
- Shot quality blends **separation** (space from the defender / beating them off balance),
  **timing** (release near the perfect zone of the repeating rhythm meter under your feet),
  **stability** (don't be flying sideways at release), a small **heat** bonus, and a
  **pressure** penalty. It must clear a required bar (~0.62, rising with heat) to score,
  and ≥0.86 to swish.

## Structure

This is a **pure-TypeScript leaf app** over the shared engine. There is no
`Cargo.toml` / `app.toml` / `package.json`: it is not a cargo workspace member and
`cargo xtask check-architecture` does not classify it. Everything lives under `web/`:

- `web/src/{constants,vec,types,gameplay,session,meshgen}.ts` — the **SDK-free** core.
  All gameplay math is pure and deterministic (no wall-clock, no RNG — shot variation
  is derived from the shot number), so the whole game is constructible and replayable
  under bare `node --test`. `web/src/heat-check.test.ts` covers it.
- `web/src/scene.ts` — the ONE file that touches `@axiom/game`, building the 3D scene
  procedurally and mirroring the session's `view()` into scene nodes each frame.
- `web/src/game.ts` — registers the fixed-update loop, folds input into an `Intent`,
  advances the session, and exposes `readHud()` for the DOM overlay.
- `web/src/harness.ts` — the browser boot edge (wasm init + `boot({ present3d })` + the
  DOM HUD). Its three dev-server anchors are what the single-file packager rewrites.

## Run

```sh
# Tests (no wasm, no DOM):
node --test apps/axiom-heat-check/web/src/heat-check.test.ts

# Typecheck the app:
npm --prefix packages/axiom-game exec -- tsgo -p apps/axiom-heat-check/web/tsconfig.json

# Build + package the self-hosted gallery page, then browse it:
make gallery-heat-check
make gallery            # serves dist/ at http://localhost:8000
# open http://localhost:8000/heat-check/index.html
```

The 3D present path needs a GPU; in a headless browser (no WebGPU) the canvas paints
black while the DOM HUD still renders — use a real browser to see the court.

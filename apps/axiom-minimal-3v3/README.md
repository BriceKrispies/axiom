# Minimal 3v3 Basketball

A deliberately minimal, legible 3D arcade half-court basketball game authored
**purely in TypeScript** on the `@axiom/game` SDK. You control the blue ball
handler — control transfers with the ball on a pass — against three red AI
defenders, with two blue AI wings to pass to. A third-person camera follows the
handler, aimed at the hoop.

**Controls:** WASD (or arrows) move · Q/E pass to the left/right teammate ·
hold SPACE to gather into a jump, release at the apex to shoot · R reset.

**The loop:** every possession ends in exactly one of make / miss / steal /
interception, freezes ~0.8 s on the result, and resets with you in possession.
Shot success is fully deterministic — a seeded hash of attempt number, shooter,
timing bucket, and distance bucket rolled against a chance built from release
timing (apex-relative), distance, and defender contest. PERFECT timing improves
the odds strongly but never guarantees.

## Architecture

A pure-TypeScript leaf app over the shared engine (like `apps/axiom-heat-check`,
its template): **not** a cargo workspace member; everything lives under `web/`.

- `web/src/vec.ts`, `constants.ts`, `types.ts`, `gameplay.ts`, `session.ts`,
  `meshgen.ts` — the SDK-free deterministic core (state machine, shot formula,
  AI targets, arcs). No wasm, no DOM, no `@axiom/game`.
- `web/src/scene.ts` — the ONE file that touches the engine: procedural court /
  hoop / six box-and-sphere figures via the SDK's 3D scene surface, plus the
  follow camera (presentation smoothing, not game state).
- `web/src/game.ts` — the `onFixedUpdate` glue: keyboard → `Intent` → session →
  scene.
- `web/src/harness.ts` — the browser platform edge: wasm init, `boot()`, DOM HUD.

Apps sit outside the engine's branchless + coverage gates; the core is still
covered by a `node --test` suite (determinism/replay hash, shot formula,
turnovers, control transfer, defender sanity).

## Run

```sh
# Tests (SDK-free core; no build needed):
node --test apps/axiom-minimal-3v3/web/src/minimal-3v3.test.ts

# Typecheck + compile to web/dist (requires the SDK built once):
npm --prefix packages/axiom-game exec -- tsgo -p apps/axiom-minimal-3v3/web/tsconfig.json

# Regenerate the self-contained gallery page, then browse it:
make gallery-minimal-3v3
make gallery          # http://localhost:8000/minimal-3v3/index.html
```

Headless/automation note: force the software backend with
`?backend=canvas2d` (headless Chrome's WebGPU path panics).

# Headless integration proofs — SDK primitives over the real wasm

**Out-of-gate** integration tests that drive the real `axiom-game-runtime` wasm in
Node (no browser) to prove SDK primitives end to end across the live boundary.
They are NOT part of the @axiom/game coverage gate (the node:test unit suite never
loads wasm); they need the wasm-bindgen `pkg/` bindings built (see below).

## `headless-fps.test.mjs` — FPS primitives

Proves the SDK's first-person primitives end to end:

- **Look-at camera** — `setCamera3D(position, target, …)` aims the camera; the
  `target` now crosses the wasm boundary (previously dropped).
- **Headless input + loop driver** — `headless()` (`packages/axiom-game/src/headless.ts`),
  the non-browser analogue of `boot.ts`: a caller hand-cranks `stepTicks` and
  injects input programmatically, exactly as an agent would.

This is the headless counterpart of the browser `web/` harness that exercises
`boot.ts`. Both `boot.ts` and `headless.ts` are platform edges (coverage-exempt,
live wasm), so they are proven here / via Playwright rather than in the node:test
coverage unit suite — which never loads wasm.

### What `headless-fps.test.mjs` asserts

1. Programmatic input injection reaches the native input system and surfaces to
   the author (`sim.input.axis` tracks the injected `ArrowRight` hold/release).
2. The driver steps the deterministic loop (`stepTicks` ⇒ `currentTick`).
3. The look-at camera call (`setCamera3D` with a per-tick moving target) executes
   live across the real wasm boundary every tick without throwing. (Camera
   *correctness* — orientation tracks the target — is proven natively in
   `apps/axiom-game-runtime/src/scene3d.rs`.)
4. Two identical runs are byte-for-byte deterministic (author-visible axis
   sequence + the native sim snapshot).

## `draw2d-marshalling.test.mjs` — SPEC-04 Frame 2D primitives

Proves SPEC-04 §7's headless obligation: the SDK's `Frame` 2D methods marshal to
the native `Draw2dList`. It binds the real wasm host, drives a few `Frame` draws
(`rect`/`circle`/`line` with fill/stroke/layer/alpha) submitted out of layer
order, calls `frame.finish()`, and asserts the returned flat, self-describing
`[kind, layer, submission, len, …geometry]` stream (see
`apps/axiom-game-runtime/src/draw2d.rs` `draw2d_finish`):

1. **Layer-sort golden** — commands come back stably sorted by `(layer,
   submission)`, including a within-layer tie that keeps its submission order.
2. **Geometry + colour marshalling** — each primitive's payload matches the
   native per-kind layout, with the SDK's `[r, g, b, a]` colours round-tripping
   through the boundary's packed `0xRRGGBBAA`.
3. **Determinism** — the same draws yield a byte-identical command stream.

## Build the bindings and run

The `pkg/` dir is generated wasm-bindgen output and is git-ignored (`**/pkg/`).
Regenerate it whenever the runtime changes:

```sh
# 1. build the runtime for wasm
cargo build -p axiom-game-runtime --target wasm32-unknown-unknown

# 2. generate Node (CommonJS) bindings into ./pkg
wasm-bindgen --target nodejs \
  --out-dir apps/axiom-game-runtime/integration/pkg \
  target/wasm32-unknown-unknown/debug/axiom_game_runtime.wasm

# 3. run the proofs (Node 24+ runs the TypeScript SDK sources directly)
node --test "apps/axiom-game-runtime/integration/*.test.mjs"
```

## Note: the Node audio stub

The browser-targeted wasm opens a Web Audio `AudioContext` on the first `advance`
to realize its sound batch — a browser sink Node lacks and the headless path never
uses. The test installs a minimal `globalThis.AudioContext` stub (the few methods
`realize_into` touches with an empty batch) so driving the loop in Node doesn't
trip on the audio side effect. The stub asserts nothing; it only stands in for the
absent browser audio output. A future native-audio backend would remove the need
for it.

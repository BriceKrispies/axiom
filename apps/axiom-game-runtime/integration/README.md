# Headless integration proof — SDK FPS primitives

An **out-of-gate** integration test that drives the real `axiom-game-runtime`
wasm in Node (no browser) to prove the SDK's first-person primitives end to end:

- **Look-at camera** — `setCamera3D(position, target, …)` aims the camera; the
  `target` now crosses the wasm boundary (previously dropped).
- **Headless input + loop driver** — `headless()` (`packages/axiom-game/src/headless.ts`),
  the non-browser analogue of `boot.ts`: a caller hand-cranks `stepTicks` and
  injects input programmatically, exactly as an agent would.

This is the headless counterpart of the browser `web/` harness that exercises
`boot.ts`. Both `boot.ts` and `headless.ts` are platform edges (coverage-exempt,
live wasm), so they are proven here / via Playwright rather than in the node:test
coverage unit suite — which never loads wasm.

## What `headless-fps.test.mjs` asserts

1. Programmatic input injection reaches the native input system and surfaces to
   the author (`sim.input.axis` tracks the injected `ArrowRight` hold/release).
2. The driver steps the deterministic loop (`stepTicks` ⇒ `currentTick`).
3. The look-at camera call (`setCamera3D` with a per-tick moving target) executes
   live across the real wasm boundary every tick without throwing. (Camera
   *correctness* — orientation tracks the target — is proven natively in
   `apps/axiom-game-runtime/src/scene3d.rs`.)
4. Two identical runs are byte-for-byte deterministic (author-visible axis
   sequence + the native sim snapshot).

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

# 3. run the proof (Node 24+ runs the TypeScript SDK sources directly)
node --test apps/axiom-game-runtime/integration/headless-fps.test.mjs
```

## Note: the Node audio stub

The browser-targeted wasm opens a Web Audio `AudioContext` on the first `advance`
to realize its sound batch — a browser sink Node lacks and the headless path never
uses. The test installs a minimal `globalThis.AudioContext` stub (the few methods
`realize_into` touches with an empty batch) so driving the loop in Node doesn't
trip on the audio side effect. The stub asserts nothing; it only stands in for the
absent browser audio output. A future native-audio backend would remove the need
for it.

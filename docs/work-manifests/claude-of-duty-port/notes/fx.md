# fx port notes

Slice: `apps/claude-of-duty/src/fx/`, ported from Claude-of-Duty `src/fx/*.js`
(all files except `ambience.js`, which is a separate, not-yet-ported slice, and
`preview.js`/`preview.html`/`shoot.mjs`, the source's own dev harness).

## What was ported

| this module | source |
|---|---|
| `fx::noise` | `fx/noise.js` (fx-local Perlin/Worley — a different implementation from `materials::noise`'s GLSL hash-lattice noise; see its module doc) |
| `fx::util` | `fx/util.js` (out-parameters dropped — see module doc) |
| `fx::particles` | `fx/particles.js` — the ring buffer + `emit`/`flush` are golden-captured; the closed-form integration is a documented transcription with property-test pinning (no JS function exists to import — see below) |
| `fx::atlas` | `fx/atlas.js` — all 16 particle painters + 16 decal painters, byte-for-byte |
| `fx::decals` | `fx/decals.js` — Sutherland–Hodgman clip, ring buffer |
| `fx::shells` | `fx/shells.js` — the fallback ballistic path only (see gaps) |
| `fx::tracers` | `fx/tracers.js` |
| `fx::lights` | `fx/lights.js` |
| `fx::haze` | `fx/haze.js` — CPU emit + resize math only |
| `fx::explosions` | `fx/explosions.js` |
| `fx::muzzle` | `fx/muzzle.js` |
| `fx::impacts` | `fx/impacts.js` — all 12 surface recipes + `spark`/`bulletHole` |
| `fx::system` | `fx/index.js` — the CPU-testable facade (budgets, emit dispatch, public API) |
| `fx::world` | not a port — the physics seam every one of the above needs |

`fx::mod` re-exports all of the above; `src/lib.rs` gained one `pub mod fx;`
line.

## Surface enum reuse

`fx::impacts` dispatches on `crate::world::palette::Surface` (already ported,
12 variants) rather than defining a second surface tag, per the task brief.
`Surface`'s declaration order already matches `SURFACE_NAMES`
(`physics/surfaces.js`), so no remapping table was needed.

## Verification (golden-capture)

`tests/fx/capture.mjs` runs the **original** `fx/particles.js`, `fx/decals.js`,
`fx/atlas.js`, `fx/impacts.js` under Node (three.js resolved from the source
repo's own `node_modules`), writes `tests/fx/golden.json`, and
`tests/fx_port.rs` pins against it:

- **Particle emission** — the real `ParticleLayer.emit()`'s raw interleaved
  record, for a 6-particle RNG-seeded sequence over a fixed `now` schedule.
  Exact `f32` equality (widened to `f64`).
- **Decal ring-buffer eviction** — `DecalSystem.add()`'s cursor/wrapped state
  and first written vertex, at and one past an 8-decal budget. Exact.
- **Particle and decal atlas bakes** — the full byte buffer of a 32px bake.
  `albedo`/`orm` are exact; `normal` allows a ±1 per-byte tolerance (2 of 4096
  bytes differ by exactly 1) because its final byte comes through a
  `sqrt`-based `normalize3` (mirroring `Math.hypot(...) || 1`), which this
  codebase's established convention (see `tests/audio_port.rs`) does not
  treat as bit-guaranteed across V8 and Rust's libm.
- **Per-surface impact selection, all 12 surfaces** — for each surface, the
  exact sequence of additive/lit particle-tile ids `spawnImpact` emits
  (recorded through a JS stub mirroring `FxSystem`'s `emitAdd`/`emitLit`
  contract) and the resulting decal count. All 12 pass exactly, including
  `metal`'s recursive `spark()` bounce chain. Every pool-budget assertion
  (`add_count <= capacity`, `lit_count <= capacity`) holds for every surface.

`src/fx/particles.rs`'s own unit tests additionally pin
`ParticleLayer`/`DecalSystem` budget-never-exceeded and dirty-range behaviour
directly (not just through the golden), and `src/fx/system.rs`'s tests pin the
budget arithmetic (`particleBudget` 2000/4000/12000/24000 →
`particle_budget/decal_budget` capacities, `decalBudget` 64/128/256/512) from
`config.js`.

### Why the closed-form particle integration is *not* golden-captured

`particles.js`'s `PARTICLE_VERT` (the vertex shader that actually integrates
position/velocity/colour/alpha every frame) exists **only as a GLSL string** —
there is no JavaScript function that performs this computation and could be
imported and called. `fx::particles::integrate` is a direct, line-cited
transcription of that GLSL into Rust instead, and is pinned by property tests:
birth/death boundary conditions, and a comparison against small-step
semi-implicit-Euler numerical integration of the `dv/dt = -k v + g` ODE the
source's own doc comment states as the closed form it solves. This is
documented in `fx::particles`'s module doc as a deliberate divergence from the
golden-capture recipe, not an oversight.

## Divergences and source quirks preserved

- `smoothstep(a, b, x)` in `fx::noise`: the JS divisor is `(b - a) || 1e-6` —
  falls back to `1e-6` **only when `b - a` is exactly zero**, not whenever it
  is small or negative. A naive `.max(1e-6)` would also clamp every reversed
  edge pair (`a > b`, used throughout `atlas.js` to invert a falloff
  direction) and flip its sign. Ported exactly; caught before it ever reached
  a test.
- `ParticleLayer`/`DecalSystem`'s ring cursor sets `wrapped = true` on the
  write that **fills** the last slot, not the write past it — confirmed
  against the real JS in the golden capture (`decalsEviction[7].wrapped ==
  true` after exactly 8 adds to an 8-slot ring).
- `MUZZLE_PROFILES` lookup (`profile_for`) matches the **first** substring hit
  in declaration order — `"suppressed_pistol"` resolves to the `pistol`
  profile, not `suppressed`, because `pistol` is scanned first. Pinned as its
  own test (`profile_for_matches_the_first_substring_in_declaration_order`).
- `water()`/`ground()` accept an `inc`/`incident` parameter the source's body
  never reads; `glass()` accepts `e`/`energy` it never reads. Ported as
  narrower Rust signatures (the unused parameter dropped) rather than kept as
  dead parameters, since Rust warns on unused args and the source's own
  behaviour is unaffected either way — noted here rather than silently.

## Gaps and seams (documented, not shortcuts)

- **`ambience.js` is unported** (separate slice). `FxSystem::add_smoke_column`/
  `add_smoke_source`/`remove_smoke_source` are no-op stubs. This means the
  port's RNG stream is self-consistent *within fx* but will diverge from the
  real game's stream once `ambience.js`'s own (currently unknown) RNG draws
  would have fired — documented in `fx::system`'s module doc with the exact
  spot (`ShellSystem::new`) a future `Ambience` port should fork from.
- **The physics seam (`fx::world::FxWorld`)**: decals' triangle-soup query and
  impacts'/system's raycast/ground-probe calls take a trait, not
  `crate::physics::bvh::StaticWorld` directly, because `StaticWorld` does not
  yet expose raw triangle vertex positions publicly (only `node_bounds`,
  `normal_of`, `surface_of`, `query_aabb`). A small, additive accessor on
  `StaticWorld` (e.g. `triangle(tri) -> ([[f64;3];3], [f64;3])`) would let a
  future integration pass implement `FxWorld` directly — not done here since
  `crates/physics` is outside this slice.
- **Shell rigid bodies**: only the fallback ballistic-arc-plus-tumble
  integration path is ported (`shells.js:230-238`). The `physics.
  addRigidBody` path needs a real rigid-body simulation this port does not
  have; `ShellSystem::spawn` always takes the fallback path here. The brass
  texture bake (`shells.js:58`, one `rng.fork()`) is still performed and
  stored, preserving draw order.
- **The GPU/presentation seam**: every `THREE.*` object (buffers, materials,
  meshes, render targets), every shader source string
  (`PARTICLE_VERT`/`PARTICLE_FRAG`, `DISTORT_FRAG`/`WARP_FRAG`), and every
  camera/scene-graph read (`fx::index.js`'s `_syncLighting`, `viewFlash`,
  `muzzleFlash`'s view-space conversion, `prewarmMaterials`, the whole
  `debugBurst`/`_findTarget` screenshot-staging harness) is unported. Each is
  called out at its exact site in the relevant module's doc comment. This
  mirrors the audio port's `web_sys`-only-on-`wasm32` seam.
- `muzzle::screen_angle` takes the camera's view-space right/up basis as a
  parameter (`Option<([f64;3],[f64;3])>`) instead of reading a live camera,
  returning `0.0` when `None` — matching the source's `if (!cam) return 0`.
  Whoever lands the camera/viewmodel integration wires the real basis in.
- `fx::system::FxSystem::sun_world`/`view_flash`'s `key` are not read live
  from a renderer (`render?.sunDir`, `render?.viewSun?.intensity`) since none
  exists yet; `set_sun_world` and a `key` parameter stand in, defaulting to
  the source's own fallbacks (straight up, `2.5`).

## Verify

- `cargo test -p axiom-claude-of-duty` — pass (fx unit tests: 75; golden:
  5/5; no regressions in `weapons`/`world`/`physics`/`materials`/`audio`).
- `cargo xtask check-architecture` — pass.

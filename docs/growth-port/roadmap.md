# Roadmap — building the target on Axiom

A phased order derived from the gaps ([`gap-analysis.md`](gap-analysis.md)) and Axiom's laws. The guiding principle, from Risk R1: **build in an app, prove it, then graduate stable primitives down into modules/layers.** Pay the branchless + 100%-coverage tax only on code that has earned permanence.

This roadmap is deliberately ordered to **de-risk the engine-capability unknowns first** (dynamic meshes, cooperative gen) — *before* gameplay, because the target's own lesson is that the substrate is the hard part and the loop is downstream.

## Phase 0 — Prove the two engine unknowns (spikes, app-level)
Nothing here is the game yet; it removes the two risks that could invalidate the whole approach.

1. **Dynamic-mesh streaming spike (Risk R2).** One app: a single chunk-sized mesh whose vertices regenerate every second and upload to the live wgpu surface; then N of them appearing/disappearing on a timer. Proves `resources`→`webgpu`→`windowing` can stream and free geometry per frame in the browser. **Exit:** visibly updating, non-baked terrain-like mesh at interactive frame rate.
2. **Cooperative gen-job spike (Risk R3).** A deterministic job that does "work" across many `step()` ticks with progress + cancel, no threads. **Exit:** a multi-second computation amortised over ticks, replayable, cancellable.

If either spike fails cleanly, that is a design signal about deployment target (Risk R4) — resolve before Phase 1.

## Phase 1 — Determinism & generation primitives (graduate to layers/modules)
These are pure, testable, branchless-friendly, and reusable — they belong in the spine.

3. **Float/distribution RNG** → extend `axiom-kernel` `DeterministicRng` (range f32, uniform, unit-vector-on-sphere). *(layer: kernel)*
4. **Noise layer** (`layer:noise`): value/Perlin/Simplex + FBM + domain warp, seeded, branchless. *(depends on math + kernel RNG)*
5. **Spherical/geo math** (extend `axiom-math` or new `layer:geo`): lat/long, great-circle, tangent frames, unit-dir ↔ region.
6. **Icosphere topology** (`module:planet-topology`): construction, subdivision levels, half-edge / region-neighbour CSR graph, BFS distance/label. *(this is the substrate every later stage reads)*

Each ships with 100% coverage. This is where Axiom's discipline is an asset, not a tax: these functions are arithmetic/table-shaped and replay-critical.

## Phase 2 — Overworld atlas (the durable product core)
Mirrors Growth milestone **M0 (overworld half)**.

7. **Worldgen feature module** (`feature-module:worldgen`): a **data-ordered stage pipeline** running tectonics → elevation(+noise) → erosion(stream-power, as a cooperative job from Phase 0) → land-fit → moisture → rivers. Start the *stage list as data* from day one (Risk R6 / moddability).
8. **Planet atlas module** (`module:planet-atlas`): the queryable output — per-region plate/elev/moisture/pos + CSR neighbours, **spatial-indexed `locate_region`**, `sample_surface`. This is the contract the game reads.
9. **QA gates as workspace tests**: determinism hash, topology-ring validity, land-fraction within tolerance — port Growth's `worldgen_bench` gates to native Rust tests.

**Vertical slice A (recommended first real milestone):** *seed → cooperative gen → atlas → render the planet as a debug globe in the browser.* This is the analogue of Growth's `SpherePreview` and is a complete, demoable, deterministic artifact. It does **not** require dynamic streaming or gameplay — a good early win.

## Phase 3 — Game-world streaming (the engine-hard part)
Mirrors Growth milestone **M0 (game-world half)**. Depends on Phase 0 spike #1 succeeding.

10. **Game-world module** (`module:game-world` / `feature-module:game-world`): `GameWorldLocalMap`, atlas-seeded per-chunk pipeline (`sample_macro`/IDW → `base_height` → `detail_noise` → `build_height_grid`), chunk store, focus-radius load + unload, seam continuity.
11. **App composition**: player rig (reuse `scene` FPS controller + retro FPS-style input wiring) standing on streamed chunks; per-frame cull with `math::Frustum`; dynamic chunk meshes via the Phase-0 path.

**Vertical slice B:** *Start → drop player onto streamed, overworld-shaped terrain you can walk around.* This is exactly the milestone the Growth repo itself has *not yet visually proven* — reaching it on Axiom would already match Growth's current real state, on a stricter foundation.

## Phase 4 — First gameplay verb (app-owned)
Mirrors Growth **M1**.

12. **Dig/terraform**: interaction ray → intent on a command queue → cell mutation in `game-world` → diff → mesh patch. Sim-owned inventory (ECS columns). This is the first genuine *loop* (dig → collect → place).

## Phase 5+ — Downstream gameplay (app-owned, graduate as patterns stabilise)
Mirrors Growth **M2–M8 / North star**, same ordering and same caveats: survival needs/threats → guardrailed emergence (bias data) → spirit/possession + sim-time gate (maps onto `SimulationClock` advance control) → ecology (template species → regional pops → local spawn) → presentation (cel materials/lighting/biome tint — a render feature-module extension, since basic-lit is all that exists today).

Keep these in the **app** until a pattern is proven reusable; only then graduate it to a module. Resist building gameplay modules speculatively — the target product itself hasn't pinned its core loop (see the scope note in [`target-product.md`](target-product.md)), so over-investing in gameplay-module structure early would repeat the very risk worth avoiding.

## Placement summary (target → Axiom)

| Target subsystem | Axiom placement | Phase |
|------------------|-----------------|-------|
| Float/dist RNG | `kernel` | 1 |
| Noise (FBM/Perlin/Simplex) | `layer:noise` | 1 |
| Spherical/geo math | `layer:math`/`geo` | 1 |
| Icosphere + region graph | `module:planet-topology` | 1 |
| Worldgen stage pipeline | `feature-module:worldgen` | 2 |
| Surface atlas + queries | `module:planet-atlas` | 2 |
| Worldgen QA gates | workspace tests | 2 |
| Game-world streaming/chunks | `module:game-world` | 3 |
| Dynamic mesh upload path | `resources`/`webgpu`/`windowing` | 0 (spike), hardened 3 |
| Cooperative gen job | app pattern → module | 0/2 |
| Player/input/interaction | `app` (+ later `feature-module:input`) | 3–4 |
| Dig/inventory/survival/spirit/ecology | `app` ECS systems, graduate later | 4–5 |
| Cel materials / terrain shaders | `feature-module:render-*` | 5 |
| Moddable defs (pipelines/presets/biomes) | `app`/`tool` → `feature-module:defs` | 2+ |

## Definition of "proven" (so this can be checked, not just believed)
The original ask was to be able to **prove** the features exist. On Axiom that proof is native and strong: each graduated primitive ships at **100% coverage** with deterministic replay tests; each worldgen QA gate is a workspace test; each browser-visible slice (debug globe, streamed terrain, dig) is confirmed with the Playwright controller (`scripts/playwright_controller.py`) and a screenshot. "Implemented" means: in the right layer/module, branchless where required, fully covered, deterministic, and demoed — not merely present.

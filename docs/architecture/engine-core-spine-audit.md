# Engine-Core Spine Audit

An audit of the Axiom workspace against the target engine-core spine (kernel /
runtime / math / frame / host layers; scene / resources / render / animation /
physics / input / audio modules), performed on the `chore/engine-core-spine`
branch.

**Headline finding:** the spine already exists and is mechanically enforced.
All five target layers are present; six of the seven target modules already
exist as isolated engine modules. The only genuinely-missing target module was
`axiom-animation`, which this pass adds. See
[`engine-core-spine-plan.md`](engine-core-spine-plan.md) for the changes.

## 1. Current workspace inventory

The workspace is partitioned into six package classes, all mechanically
classified by `cargo run -p xtask -- check-architecture`:

- **Layers** — `crates/*` with a `layer.toml` (the layer DAG).
- **Engine / feature modules** — `modules/*` with a `module.toml`.
- **Apps** — `apps/*` with an `app.toml` (leaf composition roots).
- **Games** — `games/*` with a `game.toml` (cartridge tier a host app loads).
- **Tools** — `tools/*` and `xtask` (repo tooling, off the runtime graph).
- **Support** — `crates/axiom-zones` (build-time zone-marker proc-macros).

## 2. Current layers (`crates/*`)

16 layers, DAG rooted at the kernel. Target layers in **bold**:

| Layer | `depends_on` | Role |
|-------|--------------|------|
| **kernel** | — (root) | Deterministic substrate: time/ticks, ids, errors, dimensioned scalars, telemetry. |
| **runtime** | kernel | Deterministic lifecycle, fixed-step stepping, scheduling, replay records. |
| **math** | kernel, runtime | Vectors/quaternions/Mat4/transform/geometry (the dimensionless scalar layer). |
| **host** | kernel, runtime, math | Deterministic platform boundary + backend-neutral 2D draw contract. |
| **frame** | kernel, runtime, host | The per-frame engine contract (`EngineFrame`, `FrameBuilder`). |
| ecs | kernel, frame | Archetype ECS (`World`, queries, systems, command/event buffers). |
| crypto | kernel | Signing/verify/HMAC/JWT primitives. |
| introspect | kernel, frame, ecs | Frame/world/system/metric reports for agent interrogation. |
| interface | kernel | Generic windowing/panels/console/command-table. |
| layout | kernel, host | Responsive flex solver. |
| space, entropy, proc, proc-validate | (proc-gen substrate) | Deterministic address/entropy/recipe/validation substrate. |
| noise, geosphere, hydrology | (worldgen math) | Coherent noise, geodesic topology, graph hydrology. |

All five target layers (**kernel, runtime, math, host, frame**) are present and
valid. No layer carries gameplay/domain concepts; the domain-flavoured layer
names (`geosphere`, `hydrology`, `noise`) are generic math/topology, correctly
placed.

## 3. Current modules (`modules/*`)

37 modules. The seven **target core modules** and their status:

| Target module | Present? | Facade | Notes |
|---------------|----------|--------|-------|
| scene | ✅ | `SceneApi` (+ `SceneNodeId`) | Nodes, transforms, components, deterministic `SceneSnapshot`. |
| resources | ✅ | `ResourcesApi` | CPU mesh/material/texture + `ResolvedResources`. No skeleton/clip handles (see §8). |
| render | ✅ | `RenderApi` | `RenderInput` → deterministic `RenderCommandList`; backend-neutral. |
| **animation** | ✅ *(added this pass)* | `AnimationApi` (+ `SkeletonId`/`ClipId`/`BoneId`) | Skeletons, poses, clip sampling, blending. |
| physics | ✅ | `PhysicsApi` (+ handles) | Deterministic rigid bodies, colliders, contacts, snapshots. |
| input | ✅ | `InputState` (+ ids) | Raw device frames → deterministic action snapshot. |
| audio | ✅ | `AudioApi` | Neutral synthesis/mix command model. |

All six pre-existing core modules are isolated engine modules
(`allowed_modules = []`) and pass the Module Law. The remaining ~30 modules are
other engine capabilities (`tick`, `tween`, `grid`, `draw2d`, `assets`,
`recording`, `sim-core`, netcode/net-protocol/client-core, the GPU/canvas/
windowing backends, `debug-overlay`) plus the worldgen/agent modules noted in §6.

## 4. Current apps (`apps/*`)

12 apps, each an `app.toml` leaf composing declared layers + modules — e.g.
`axiom-demo-rotating-cube` (the vertical-slice render demo), `axiom-gallery`
(the showcase host, which also loads the `retro_fps` game cartridge),
`axiom-game-runtime`, `axiom-worldgen-demo`, and the netcode/sim harness apps.
Apps are the only leaf composition roots and the only tier that translates
between two module contracts.

## 5. Current tools (`tools/*`)

11 tools + `xtask`: `axiom-shot` (screenshot capture), `lints` (the dylint
rulebook), `axiom-profile-runner`, `axiom-proc-fuzz`/`-inspect`, the netplay
server/relay/load tools, `axiom-asset-pack`, `axiom-dev-reload`, `wasm-runner`.
Tools are off the runtime dependency graph.

## 6. Packages that look misplaced

Several `modules/*` carry content/domain-flavoured responsibility rather than a
game-agnostic mechanism: `forest` (a chunk of trees — feature module),
`biome` (hardcoded Whittaker thresholds), `planetgen`, `levelgen`, `worldsave`,
`world` (streaming-world plan). Most are **feature modules** that compose lower
worldgen modules; they are legal under the current Module Law and are consumed
only by the worldgen-demo/gallery apps.

Per the scope decision for this pass, these are **left exactly as they are** —
no relocation and no migration plan. They are recorded here only so a future
reader knows they lean app/content-ward; whether they should graduate toward
`apps/`/`games/` is a separate decision, not part of this spine pass.

## 7. Repeated mechanisms found in apps/modules

The audit found no core *mechanism* being re-implemented across apps — the six
existing core modules already own scene/resources/render/physics/input/audio.
The one true gap was skeletal animation: no module owned skeletons/poses/clip
sampling, so any animated game would have had to build it itself. That gap is
what this pass closes with `axiom-animation`.

## 8. Missing reusable engine contracts

- **Skeletal animation** — *(closed this pass)* now `axiom-animation`.
- **Rotation interpolation in math** — *(closed this pass)* `Quat::nlerp` added
  to `axiom-math` (the layer that owns rotation maths), reused by animation.
- **Skeleton / animation-clip resource handles in `resources`** — the target
  envisages `resources` holding `SkeletonHandle`/`AnimationClipHandle`. Today
  `resources` owns only mesh/material/texture, and `axiom-animation` is
  self-contained (it defines its own skeletons/clips), so this is **not**
  required for animation to function. Left as a documented future option.

## 9. Recommended migration order

For the spine itself, no migration is required — it is already in place. The
only structural addition is the one made this pass (animation module + the math
primitive it needs). Any future content-module relocation (§6) would be
sequenced after this and is out of scope here.

## 10. Risks and compatibility notes

- **Additive only.** This pass adds one module and one math method; it renames,
  moves, or deletes nothing. No existing app or module changes, so nothing that
  compiles today stops compiling.
- **No facade churn.** The existing core modules use established facade/type
  names (`InputState`, `ResourceId`, `SceneNodeId`, `PhysicsApi` handles) that
  apps already depend on. Renaming them to the target's suggested names would
  ripple through apps for zero structural gain, so it was deliberately **not**
  done — see the plan's "intentional non-changes".
- **Animation overlap.** A richer, unmerged animation module exists on a
  separate worktree branch; the minimal `axiom-animation` added here is the
  deterministic contract on `main`. Reconciling the two is future work and is
  not tracked further here, by decision.
- **All gates green.** The addition holds the workspace at 100% coverage, zero
  new dylint warnings (branchless + genuine-dependency + no-unwrap), and a
  passing architecture check.

# Axiom Engine Datafication

> How far "the engine is data" can actually go in Axiom, where turning code into
> data removes code versus adds it, and where the real code mass collapses. This
> is a **north-star and a boundary** — it names the target *and* the line past
> which "everything is data" becomes the clever-abstraction junk drawer the rest
> of `CLAUDE.md` exists to reject.

Status: **assessment landed, no engine code changed yet.** This document is the
result of a full read of the current code-vs-data boundary across proc/recipe,
ECS/scene scheduling, the render path, and the sim-core / introspect / app
substrates. It sets direction; the staged work in §7 is not yet built.

## 0. The ambition, stated precisely

The goal is: *the smallest amount of code the engine truly needs, and everything
else expressed as data.* Not just pulling constants out of code — pulling engine
**behavior** (methods, logic, pipelines, schedules) out into data executed by a
minimal core.

Taken literally and applied everywhere, that ambition is **a reflective bytecode
VM**: data that names arbitrary engine operations, dispatched at runtime. Axiom
forbids exactly that, on purpose (§5). So "as data-driven as it can be" is not
"maximal data" — it has a precise, principled ceiling, and the honest target is
different from and larger than the naive one: a **small, fixed engine** with
**everything built on it — apps, scenes, content, behavior — expressed as data.**

## 1. The one finding: data at the seams, code at the leaves

Every subsystem shows the identical shape. Axiom is already thoroughly
data-driven *between* its modules and hardcoded *inside* each leaf.

| Subsystem | Data (the seam) | Code (the leaf) |
|---|---|---|
| **proc / recipe** | `RecipeGraph` (opcode + param words + input links); `ProcCore::execute` is a ~30-line generic `try_fold` | op *bodies* (`cube`, `noise`, `meta_surface`), their param layouts, arity, clamps |
| **ECS / scene** | `FrameCommand` input bus (kind + payload); `axiom-runtime` orders systems by an explicit `i32` data key | the scene's *system set and order* — a hardcoded 5-call sequence in `Scene::new()`; system bodies |
| **render** | unbroken neutral chain `SceneSnapshot → ResolvedResources → RenderInput → RenderCommandList → GpuSubmission → FramePacket`; a branchless command interpreter feeds **two** backends | shaders / lighting model (WGSL literals), the render-pass graph, pipeline vocab, the 16-light / 5×5-PCF constants |
| **sim-core** | facts / relations / processes / effects / definitions — state and causality fully as data | the *rule* that transforms state is a Rust trait (`ProcessHandler`); only a fixed 6-verb `HandlerSpec` is data |

The dispatch mechanism is the same everywhere and is Axiom's native form of
"logic as data": a fieldless enum discriminant used as a **`const`
function-pointer table index** (`OPS[op as usize]`), never a `match`. It is a
jump table over a **closed, statically-known** vocabulary — the branchless
analogue of a bytecode interpreter.

Concretely, the strongest existing example — `ProcCore::execute`
(`crates/axiom-proc-core/src/proc_core.rs`) — is ~30 lines, owns zero operators,
is generic over the output type, and demonstrably drives both texture and mesh
generation from the same code. The op vocabulary
(`crates/axiom-proc-mesh/src/mesh_op.rs`, `.../texture_op.rs`) is the data; the
executor is the tiny fixed core. This is the template every other datafication in
Axiom should resemble.

## 2. The Datafication Law — when code→data removes code, and when it adds it

> **Turning a method into data removes code only when it collapses `N`
> near-duplicate hardcoded variants into `(1 interpreter + N data
> descriptions)`. The saving is `(N−1) × per-variant-code` minus the
> interpreter + format + serializer overhead. When `N = 1` — a singular
> algorithm — datafication *adds* code: an interpreter of the same size plus a
> data format. Datafication is a duplication-eater, not a code-eater.**

Corollaries:

- **High `N`, boilerplate-heavy variants → datafy.** Many near-identical
  shaders, entity kinds, levels, or per-game setups collapse dramatically.
- **`N = 1`, genuine algorithm → keep it code.** One marching-cubes, one
  transform propagation, one PCF loop. Turning these into interpreted data buys
  no determinism (the pipeline is already replay-deterministic) and costs the
  zero-cost typed path and readability.
- **The saving lives where the repetition lives.** If a tier has little
  duplication, datafying it makes it *bigger*, not smaller.

This law is why the engine spine is the *wrong* place to chase code reduction and
the app/content tier is the *right* place (§6).

## 3. The method→data dividing line

For an individual method, the test is sharp:

- **A method that is "select params + apply a formula over a closed
  vocabulary" wants to be data.** Its variation is a small set of cases; the
  cases become data, the selector becomes a jump table. (proc op param-schemas;
  the scene schedule; a material's BRDF/pipeline variant; the lighting model's
  tunable constants.)
- **A method that is a genuine algorithm stays code.** Its "variation" is
  unbounded control flow over real computation, not a case table. (marching
  cubes in `implicit.rs`; near-plane clipping / culling in
  `frame_packet_raster.rs`; `TransformPropagation`; the PCF sample loop in the
  WGSL.)

If you cannot name the closed vocabulary a method selects over, it is an
algorithm — leave it in Rust.

## 4. What "data-driven" means in Axiom specifically

Because of the laws in §5, Axiom's data-drivenness has one concrete shape, and
every datafication must take it:

- **A closed op vocabulary** (a fieldless `#[repr(uN)]` enum whose discriminant
  is the opcode).
- **POD data carried across a neutral seam** (opcode + typed-view param words +
  input links; no behavior, no engine types embedded).
- **A tiny generic interpreter** that dispatches by **`const` function-pointer
  table indexed by the discriminant** — branchless, no `match`, out-of-range
  index fails cleanly via `OPS.get(i)`.
- **Determinism as a first-class property** — positional entropy keying,
  canonical bytes, `StableHash` digests, tests asserting byte-identical replay.

Anything that cannot be expressed this way (open/dynamic op sets, runtime type
dispatch, data that names arbitrary engine methods) is **out of bounds** — not
because it is hard, but because it violates the laws below.

## 5. The principled ceiling (why the naive VM is banned)

Axiom's existing, mechanically-enforced laws *define* how far data-driving can
go. They are not obstacles to route around; they are the shape of the answer.

- **No runtime type reflection.** The `engine_no_runtime_type_branch` dylint
  bans `TypeId` / `Any` / `downcast` in engine code. `Reflect`
  (`crates/axiom-kernel/src/reflect.rs`) is a compile-time associated `const` on
  a generic bound, deliberately *not* trait objects. The ECS is named-column
  with no type registry. ⇒ **Data may not name engine operations for runtime
  dynamic dispatch.** A capability "registry" must be a *static* `const`
  catalog, not a runtime dispatch map.
- **Branchless spine** (`engine_no_branching`, baseline 0). ⇒ The interpreter
  must be a branchless table, which only works over a **closed** vocabulary.
- **100% coverage** (the Coverage Law). ⇒ Every data-selectable path needs a
  test; an open/dynamic op space is untestable by construction.
- **Layer + Module Laws.** ⇒ Data crosses module boundaries as neutral
  contracts; the *translation* between contracts lives in an app or feature
  module, never smuggled into an engine module.

The reflective VM the naive ambition implies would need runtime type machinery
(banned), unbounded dispatch (unbranchable), and an open op space (uncoverable).
Its absence is a feature: it is the difference between a durable engine and soup.

## 6. Where the code actually is — and where it collapses

Non-test Rust, by tier (measured):

| Tier | LOC | character | datafication verdict |
|---|---|---|---|
| crates (layers) | ~48,900 | singular algorithms + neutral data contracts | **already near-minimal**; laws squeezed the duplication out |
| modules | ~94,800 | singular algorithms (physics, marching cubes, rasterizer, …) | mostly `N = 1`; datafying *adds* code |
| **apps** | **~49,600** | **orchestration + content** (gallery alone ≈ 30,400) | **high `N`, high boilerplate — this is where code collapses** |
| tools | ~8,100 | repo tooling | out of scope |

The spine (crates + modules ≈ 144k LOC) is close to minimal *for what it does*:
one lighting model (a single ~240-line WGSL in
`modules/axiom-gpu-backend/src/scene_renderer.rs`), one transform propagation,
one marching-cubes, one PCF loop. The no-duplication / branchless / coverage /
module-isolation laws have **already** removed the redundancy that datafication
eats. **There is no large hidden mass of redundant spine logic waiting to
collapse into data.** That is why spine-level datafication refactors are small
and largely code-neutral.

The code that a data-driven engine actually *deletes* lives in two places:

1. **Apps + content (the ~50k app tier, and every future game).** Per-game
   setup, per-scene wiring, per-demo glue — the high-`N`, boilerplate mass. The
   engine already proved the collapse: the rotating-cube scene exists as **111
   lines of TOML** (`apps/axiom-gallery/tests/rotating_cube/package/scenes/main.toml`)
   that a runner *could* execute, versus the imperative Rust that draws the same
   scene. Make every game/demo a **data package over one runtime** and the app
   tier collapses toward `data + one small shared runner`.
2. **Future variant-heavy leaves.** The single lighting model is `N = 1`
   *today*. The moment the engine wants a second and third material model, the
   choice is "three hardcoded WGSL strings" (code grows linearly) or "one
   parameterized model + data" (code stays flat). Datafying the shader/material
   model and the render-pass graph does not shrink today's code — it **caps
   future growth**, which over the engine's life removes far more than it costs.

## 7. The staged path (ranked by leverage, honest about code impact)

Each stage is labelled with what it actually buys: **quality** (behavior becomes
declared/checkable/introspectable data, ~0 net LOC), **dedup** (removes an
accidental parallel implementation), **cap** (flattens future growth), or
**collapse** (removes a large existing code mass).

### Enabling plumbing — do these because the runner/registry consume them

- **#1 Scene schedule → data (quality).** Lift `axiom-runtime`'s proven
  explicit-`i32`-order, duplicate-rejecting scheduler
  (`crates/axiom-runtime/src/runtime_scheduler.rs`) into `axiom-ecs::World`, so
  the scene's hardcoded 5-system sequence (`modules/axiom-scene/src/scene.rs`)
  and its "must run last" doc-comment invariants become a declared, checkable
  order key. Blast radius is one production file; the branchless idiom copies
  verbatim. Net LOC ≈ 0 — this is a *checkability* win, not a size win.
- **#2B Op param-schemas → data (quality).** Replace ~23 inline `p.len() >= N`
  checks + doc-comment arity in `axiom-proc-mesh` / `axiom-proc-texture` with a
  `const` param-schema table parallel to the dispatch table. Byte-identical
  output, no consumer edits. Note: `Param` is untyped bits by design, so a
  schema can enforce **arity/stride**, not per-slot type — this is
  documentation-as-data, not new type-safety.

### Dedup — real spine code removed

- **#2A Converge the two recipe cores (dedup, ≈ −1,087 LOC).** `axiom-proc` +
  `axiom-proc-validate` (a 4-op `u64`-word interpreter) and
  `axiom-recipe` + `axiom-proc-core` (the typed-`Param` interpreter) are fully
  disjoint parallel stacks. The old one's 4 ops are trivially expressible in the
  new `RecipeGraph` model (a test already does it in ~8 lines). But it is a
  **migration, not a delete** — the old stack still ships on two golden'd paths
  (`axiom-placement` → `axiom-levelgen`, gallery quintet) via `evaluate → words`.
  Gated on human calls: keep or drop `ProcTrace` / resumable `Evaluation` /
  `Constraint`/`repair` (only demo-tooling uses them). Scope separately.

### The backbone — the keystone the collapse hangs off

- **#4 Static capability registry.** Today the reflection surface describes
  *shapes* (`TypeSchema` = field names + type-name strings, hand-curated per
  facade in `SceneApi::component_schemas`) and *frame telemetry*
  (`WorldReport` = two integers) — **not callable capabilities**. Extend
  `Reflect` / `TypeSchema` into a `const` **capability catalog** (engine ops +
  their param schemas, statically) so data can reference engine operations *the
  Axiom way* — no runtime dispatch registry. This is what satisfies Vocabulary
  Law #5 ("the spec an author reads is generated from the engine") and is the
  dispatch surface the runner binds against.

### The payoff — where the app/content mass collapses

- **App-datafication (collapse).** Design and build the never-finished
  `axiom-appc` (compiler) + `axiom-runner` (loader/executor) so the rotating-cube
  `axiom.app.toml` + scene TOML becomes the **authoritative source**, not the
  *checked mirror* it is today
  (`apps/axiom-gallery/tests/rotating_cube_scene_manifest.rs` asserts the data
  and the imperative Rust agree). The `.axpkg` target is currently a status-flag
  stub. This is the stage where the ~50k app tier — and all future content —
  starts becoming data over a fixed engine. It is the direct realization of the
  ambition and the largest code-eliminator.

### Frontier — flagged, not scheduled

- **Data-described render graph + material/lighting model (cap).** Materials
  already carry `roughness` / `emissive` the shader ignores, and an `UNLIT`
  pipeline marker is emitted but unwired. Parameterize the fixed model by data
  and select from a small closed set of variants by discriminant — **not** a
  data-described shader-graph VM (that is over the §3 line). Pays off as the
  engine grows past one model.
- **sim-core rule layer (deferred).** `axiom-sim-core` is the deepest data
  substrate (facts / relations / processes / effects / definitions), but its
  behavior slot is a deliberate stub — `ProcessHandler` is a Rust trait and only
  a 6-verb `HandlerSpec` is data (Phase 5 deferred). The Vocabulary Law says do
  not build the data-defined rule engine until a real game presents the wall.
  Flag it; do not speculate it into existence.

## 8. Current state / evidence (what exists vs. what is missing)

- **proc/recipe:** two disjoint interpreters, both real, both shipping; the new
  one is the clean template. Op param-schemas and clamps live in Rust + doc
  comments, not data.
- **ECS/scene:** `axiom-runtime` orders systems by a data key and rejects
  ambiguity; `axiom-ecs`/`axiom-scene` do not — order is insertion order plus
  prose invariants.
- **render:** every module boundary is a neutral data contract feeding two
  independent backends (GPU + software rasterizer) — genuinely data-driven
  plumbing. The rendering *semantics* (shaders, passes, lighting model, pipeline
  state) are code-fixed.
- **reflection:** `Reflect`/`TypeSchema` is real, deterministic, and
  byte-serializable, but reflects *shape and telemetry*, not *capabilities*.
  There is no capability/operation catalog — the missing keystone for #4.
- **app-datafication:** the design docs referenced by memory
  (`docs/app-datafication/`) are **gone**; there is **no** `axiom-appc` and
  **no** `axiom-runner`; `.axpkg` is a stubbed status flag. What exists is the
  rotating-cube data package as a *checked mirror* of imperative Rust, plus the
  `growth` `world_tags` path that actually loads TOML at runtime (for tags/nouns
  only, not behavior).

## 9. What "done" looks like — and what it explicitly is not

**Done:** a small, fixed engine — the interpreters and the singular algorithms —
with **everything built on it expressed as data**: games, scenes, materials,
levels, and (eventually, under proven pressure) behavior, all authored as data
packages executed by one runner, referencing engine capabilities through a
static registry. An agent authors a game with no Rust; the engine stays fully
game-neutral.

**Not done, and never the goal:** "the engine itself becomes data." The engine's
interpreters and algorithms are code by nature; datafying `N = 1` spine
algorithms *adds* code and fights the laws. The system that becomes "mostly data"
is **engine (small, fixed) + everything above it (data)** — not a spine rewrite.

## 10. Non-goals and open decisions

Non-goals (each would violate a law in §5):

- A runtime `TypeId`/`Any` dispatch registry or reflective op invocation.
- An open/extensible op vocabulary that data can add verbs to at runtime.
- A data-described shader-graph VM (parameterizing a closed model is fine; an
  open graph is not).
- Moving spine logic into an app to "become data" by dodging a gate.

Open decisions a human must ratify before the corresponding stage:

- **#1:** error surface (recommended: a dedicated `EcsError`/`EcsErrorCode`
  mirroring `axiom-runtime`, not a new kernel code); keep the two phases
  (recommended: yes, to preserve the snapshot wire format); order-source shape
  (recommended: a named `SceneSystemOrder` enum beside the systems).
- **#2A:** whether to preserve or drop `ProcTrace` / resumable `Evaluation` /
  `Constraint`/`repair`; whether re-goldening two shipping generators is worth
  the 4→2 crate merge.
- **#4 / app-datafication:** the capability-catalog schema shape, and the
  `.axpkg` + compiler/runner contract — the subject of their own design pass.

---

*This document is the durable frame. The seam-vs-leaf finding (§1), the
Datafication Law (§2), the method→data line (§3), and the ceiling (§5) are the
reasoning; §6–§7 are where the code actually collapses and in what order. When a
future change claims to "make the engine more data-driven," check it against §2
and §3 first: if it does not collapse real duplication or it datafies an
algorithm, it is adding indirection, not removing code.*

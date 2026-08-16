# Shader / Material / Field System — Coordinator Manifest

> **This directory is an execution plan for a swarm of implementation agents.**
> The architecture is already decided in [`00-architecture-findings.md`](00-architecture-findings.md).
> A subagent handed one manifest from this directory should implement exactly
> that manifest and **must not redesign anything**. If a manifest appears to
> require a decision, that is a defect in this document — report it to the
> orchestrator rather than deciding locally.
>
> **Status:** planning complete, **implementation not started**. No production
> code has been modified by the session that wrote this.
>
> **Authority:** `00-architecture-findings.md` is authoritative on *architecture*
> (what goes where, and why). **This README is authoritative on execution**:
> ordering, parallelism, and file ownership.

---

## 1. Where this directory lives

The brief asked for `work-manifests/shader-material-field-system/`. This
directory is at **`docs/work-manifests/shader-material-field-system/`** because
that is the repository's established location — `docs/work-manifests/` already
holds `curve-profile-foundation.md` and the 14-file `startup-preparation/` set,
and this plan follows their format exactly. There is no `work-manifests/` at the
repo root and creating a second convention would be its own small architectural
mess.

---

## 2. The decision, in one paragraph

Axiom is missing a **field**: a deterministic, hashable, canonically-serializable
pure function from an explicitly supplied typed evaluation context to a typed
value, represented as a closed-algebra, id-ordered, acyclic typed expression
graph. It is the engine's missing *function-as-a-value*, and it has nothing to do
with rendering — the engine has already stated the need three times in the
negative (`crates/axiom-mesh-ops/src/implicit_surface.rs:10-14`,
`crates/axiom-proc-mesh/src/implicit.rs:41-72`, and the reverted commit
`454707c0`). It belongs in a **new layer, `crates/axiom-field`**, because the
three crates that most need it are themselves layers and a layer may never depend
on a module. **Material semantics** (base colour, roughness, metallic, normal,
emission, opacity, masks, layering) are a *separate* stratum in a second new
layer, `crates/axiom-surface`, because seven engine **modules** must name a
material description and modules may never depend on modules. There is **no
render-facing shader IR** — the repository rejects it. WGSL generation, program
planning, caching and the raw-shader escape hatch all belong in
`modules/axiom-gpu-backend`, which already owns every shader string in the engine.

Read `00-architecture-findings.md` §2, §3 and §7 before touching anything.

---

## 3. Manifest index

| # | Manifest | Owner package | Wave |
|---|---|---|---|
| — | [`00-architecture-findings.md`](00-architecture-findings.md) | *(decision record — read first)* | — |
| P1 | [`P1-retire-legacy-proc-stack.md`](P1-retire-legacy-proc-stack.md) | `crates/axiom-proc*`, `modules/axiom-placement` | 1 |
| P2 | [`P2-recipe-node-diagnostics.md`](P2-recipe-node-diagnostics.md) | `crates/axiom-recipe` | 1 |
| 01 | [`01-foundational-field-ir.md`](01-foundational-field-ir.md) | `crates/axiom-field` (new) | 2 |
| 02 | [`02-field-validation-and-canonicalization.md`](02-field-validation-and-canonicalization.md) | `crates/axiom-field` | 3 |
| 03 | [`03-field-cpu-evaluator.md`](03-field-cpu-evaluator.md) | `crates/axiom-field` | 4 |
| 04 | [`04-material-semantics.md`](04-material-semantics.md) | `crates/axiom-surface` (new) | 5 |
| 05 | [`05-bake-time-field-consumers.md`](05-bake-time-field-consumers.md) | `proc-texture`, `mesh-ops`, `proc-mesh` | 5 |
| 06 | [`06-render-contract-integration.md`](06-render-contract-integration.md) | `resources`, `render`, `render-pipeline`, `host`, `axiom` | 6 |
| 07 | [`07-backend-lowering.md`](07-backend-lowering.md) | `gpu-backend`, `canvas2d-backend` | 7 |
| 08 | [`08-wgsl-generation.md`](08-wgsl-generation.md) | `gpu-backend` | 8 |
| 09 | [`09-pipeline-and-program-caching.md`](09-pipeline-and-program-caching.md) | `gpu-backend`, `axiom-runtime` barrier | 9 |
| 10 | [`10-vertex-deformation.md`](10-vertex-deformation.md) | `surface`, `gpu-backend`, `mesh-ops` | 8 |
| 11 | [`11-lighting-integration.md`](11-lighting-integration.md) | `surface`, `host`, `gpu-backend` | 8 |
| 12 | [`12-agentic-introspection-and-serialization.md`](12-agentic-introspection-and-serialization.md) | `field`, `surface`, `tools/` | 6 |
| 13 | [`13-vertical-slice-and-regression-proof.md`](13-vertical-slice-and-regression-proof.md) | `apps/burnt-rubber`, `apps/axiom-rotating-cube` | 10 |

### `05-render-shader-ir.md` does not exist, deliberately

The brief listed it. The repository rejects it — see `00-architecture-findings.md`
§3C. There is exactly one lit shader, ≤10 pipelines, no variant machinery, an
explicit twice-written anti-variant doctrine, and the only other backend cannot
execute a program at all. The two things such an IR would carry split cleanly:
backend-neutral *requirements* are derivable from the `Surface` graph and fold
into `04`; the backend-shaped *program plan* folds into `07`. The number `05` is
reused for the bake-time consumers. **Do not create a ceremonial compiler stage.**

---

## 4. Dependency graph and parallelism

```text
WAVE 1   P1 ────────────────┐        P2 ──────┐        (fully parallel with each other)
                            │                 │
WAVE 2                      │                 └──> 01 foundational-field-ir
                            │                          │
WAVE 3                      │                          v
                            │                     02 validation-and-canonicalization
WAVE 4                      │                          v
                            │                     03 cpu-evaluator
                            │                     ┌────┴─────────────┐
WAVE 5                      └────────────────> 05 bake-consumers   04 material-semantics
                                                    │                 │
WAVE 6                                              │        ┌────────┴────────┐
                                                    │        v                 v
                                                    │   06 render-contract   12 introspection
WAVE 7                                              │        v
                                                    │   07 backend-lowering
                                                    │    ┌───┴───┬───────┬──────┐
WAVE 8                                              │    v       v       v      v
                                                    │  08 wgsl  10 vtx  11 light│
WAVE 9                                              │    v                      │
                                                    │  09 caching ──────────────┘
WAVE 10                                             └──────────> 13 vertical-slice
```

**Genuinely parallel:**
* `P1` ∥ `P2` — different crates, no shared file.
* `04` ∥ `05` — `04` creates `crates/axiom-surface`; `05` edits three existing
  layers. No file overlap.
* `06` ∥ `12` — `12` is additive introspection + tooling; `06` is contract wiring.
* `08` ∥ `10` ∥ `11` — but all three touch `modules/axiom-gpu-backend`. They are
  parallel **only** if each owns disjoint files; `07` must first create the
  `surface_program/` submodule directory so the three do not contend on
  `scene_renderer.rs`. If that split is not achievable, run them sequentially
  `08 → 11 → 10`.

**Strictly sequential:** `01 → 02 → 03` (all own `crates/axiom-field/src/lib.rs`),
and `07 → 08 → 09`.

---

## 5. Rules every manifest inherits

These are repository law, verified against the enforcement code, not prose. They
are not restated in each manifest.

1. **Branchless (`engine_no_branching`, baseline 0, hard ban, no escape hatch).**
   No `if`/`else`, `match`, `for`/`while`/`loop`, `&&`/`||`, `?`, `if let`.
   *Non-negotiable consequence for this work:* **there is no data-carrying node
   enum.** The node shape is `(op: u16, params: [u32 words], inputs: [NodeId])`
   with a `const [fn; N]` dispatch table — the shape `axiom-recipe`,
   `axiom-proc-texture` and `axiom-proc-mesh` already use three times over.
   `crates/axiom-recipe/src/value.rs:5` states why in the code itself.
2. **`engine_no_recursion`, baseline 0.** The evaluator is a flat `try_fold` over
   id-ordered nodes with a register cache. No recursive descent, ever.
3. **`engine_no_large_enums` — 24 variants, deny-at-zero for a new crate.** The
   proposed algebra is 23 ops. Growth past 24 means moving to a bare `u16` code
   with a `const` catalog.
4. **`engine_no_large_structs` (24 fields), `engine_no_large_files` (1000 lines),
   `engine_no_large_functions` (120 lines), `engine_no_large_impl_blocks` (30
   items).** All effectively zero-tolerance for new crates.
5. **`engine_no_unitless_float_public_api`.** No naked `f32` on any public
   surface of `axiom-field` or `axiom-surface`. Use quantity newtypes;
   `axiom-recipe::Scalar(f32)` is the exemplar (a single-field newtype is exempt
   for its own `new`/`get`). `axiom-kernel` and `axiom-math` are the only
   exempted crates and neither is ours.
6. **`engine_no_retained_state`.** No `static`, no interior mutability, no
   `&mut self` on a public boundary, **no `F: Fn(..)` generic parameters**, no
   `Box<dyn Fn>`, no `impl Iterator`. The gate already fails on this lint with 787
   pre-existing findings — that is not permission to add the 788th.
7. **`engine_no_runtime_type_branch`.** No `TypeId`, `Any`, or `downcast`.
8. **`no_unwrap_in_engine`** — `.expect()` is the sanctioned form.
   **`engine_no_unwrap_or`** (local, uncommitted) bans `.unwrap_or(..)`; prefer
   `.unwrap_or_else` / `.unwrap_or_default` / `map_or`.
9. **`engine_require_module_docs`**, **`engine_no_wildcard_imports`**.
10. **Coverage Law: 100% regions / lines / functions** on every layer and module,
    landing **in the same change**. Apps and tools are outside the gate. For a
    table-dispatched op set this means one test reaching each operator function,
    one out-of-range-opcode test, one test per accessor and per error path.
11. **Module Law #8 facade rule.** A module's `lib.rs` may contain exactly one
    public non-`ids` line. A rich value vocabulary goes in `src/ids.rs` and is
    re-exported by a single `pub use ids::{…};` — the `axiom-figure`,
    `axiom-tween`, `axiom-world` shape. **Layers are not subject to this rule**,
    which is one more reason `field` and `surface` are layers.
12. **Platform-API hygiene.** `hygiene.rs:64` bans the substrings `web_sys`,
    `js_sys`, `wasm_bindgen`, `WebGPU`, `WebGL`, `requestAnimationFrame`,
    `window.`, `document.`, `canvas` outside the allowlist — **matched on
    comment-stripped source including string literals**. The strings `wgsl`,
    `WGSL` and `wgpu` are **not** banned. Beware bare lowercase `canvas`: a WGSL
    uniform or identifier containing it fails in any non-allowlisted crate.
13. **`SlicePlacementEngineLogicInApp`.** Do not "solve" a gate by moving engine
    logic into `apps/`. The checker exists specifically to catch that.

---

## 6. Validation commands

There is **no local commit hook**, and **CI is disabled** —
`.github/workflows/ci.yml` is `on: workflow_dispatch:` only, since 2026-07-14.
Every gate is run by hand.

```sh
cargo xtask check-architecture            # layer + module + hygiene + coverage-scope
cargo test --workspace
bash scripts/dylint-gate.sh               # see the caveat below
bash scripts/coverage.sh                  # or scripts/coverage.ps1 on Windows
bash scripts/ts-gate.sh                   # only if packages/ is touched
```

**Two caveats that will waste hours if you do not know them:**

* **Never run two gates concurrently.** Under memory pressure `dylint` reports a
  spurious `cargo metadata` error that *masks the real finding*, and `link.exe`
  fails with `0xc0000142`.
* **`scripts/dylint-gate.sh` fails today, by design.**
  `tools/lints/dylint-baseline.txt` deliberately omits `engine_no_retained_state`
  (787 known findings). "The dylint gate is green" is therefore **not** an
  available acceptance criterion. The criterion for every manifest is: *the count
  for each lint did not rise above its baseline, and the new crate contributes
  zero findings to every lint.* The gate greps `#[warn(<lint>)]` notes, which
  rustc prints **once per compilation unit** — a brand-new crate is clean by
  definition, so any finding in it trips the gate immediately.

---

## 7. What this plan explicitly does not do

* It does not build a shader-graph **VM**. `docs/engine-datafication.md:310`
  names that as a non-goal, and this design complies: a **closed** 23-op algebra
  fixed in Rust, no runtime-extensible verbs, no registry, no dynamic dispatch,
  and **no frame-time interpretation on the GPU path** — lowering and compilation
  happen at the existing `RuntimeState::Prepared` preparation barrier. See
  `00-architecture-findings.md` §5. Manifest `04` amends that document to record
  this reading. **Do not land this work silently against a written non-goal.**
* It does not build a PBR framework. `docs/specs/SPEC-11-3d-scene-surface.md`
  already says *"Resist PBR scope creep"*, and the shader is Blinn-Phong with a
  global `SPECULAR_POWER = 48.0`. `metallic` is added as a *channel*, not as a
  new lighting model.
* It does not implement marble, wood, rust, scratches, dirt, asphalt, water
  ripples, brushed metal or fabric weave **as engine primitives**. Those are
  library-authored graphs built from the 23 ops. The engine must never need a new
  Rust function for a new visual effect — that is the whole point. See
  `12-agentic-introspection-and-serialization.md` §"the library tier".
* It does not touch the TypeScript engine (`packages/axiom-web-engine`), which
  has a parallel `MaterialSpec` model, no textures at all, and a different
  shading curve. Unifying it is separate work and is **out of scope**, noted in
  `06`.

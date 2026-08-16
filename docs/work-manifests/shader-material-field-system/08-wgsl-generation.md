# 08 — WGSL generation

## Objective

Emit a WGSL fragment body from a `Surface`'s channel graphs, splice it into the
existing `SCENE_WGSL`, and prove by parity test that the generated shader agrees
with `axiom-field`'s CPU evaluator — which is the semantic reference.

## Architectural placement

**Engine module: `gpu-backend`** (`modules/axiom-gpu-backend`). Nowhere else in
the engine may hold shader text; this module already holds all seven WGSL
constants and every pipeline.

**Hygiene note, verified:** the strings `wgsl`, `WGSL` and `wgpu` are **not**
banned anywhere in the repo. `crates/xtask/src/hygiene.rs:64` bans only
`web_sys`, `js_sys`, `wasm_bindgen`, `WebGPU`, `WebGL`, `requestAnimationFrame`,
`window.`, `document.`, `canvas` — and this module is on the allowlist anyway.
**But note the bare lowercase `canvas` trap**: never name a generated WGSL
identifier or uniform something containing `canvas`, because if any helper is
ever extracted to a non-allowlisted crate the string alone fails the check.

## Existing code involved

| Path | Role |
|---|---|
| `modules/axiom-gpu-backend/src/scene_renderer.rs:29-413` | `SCENE_WGSL` — one 385-line inline string; `vs` `:180`, `vs_skinned` `:212`, `fs` `:296-412` |
| `scene_renderer.rs:73-78` | `const CAP_TEXTURES: u32 = 1u;` … — the capability masks mirrored from `frame_capability.rs` |
| `scene_renderer.rs:116` | `SPECULAR_POWER: f32 = 48.0` |
| `scene_renderer.rs:270-288` | `shadow_factor` — 5×5 PCF |
| `scene_renderer.rs:358,400,409` | hemisphere ambient; emissive added post-lighting; fog last |
| `modules/axiom-gpu-backend/src/surface_encode.rs:74-76` | `shader_source(body) = [SRGB_TRANSFER_WGSL, body].concat()` — **the only shader composition mechanism that exists**, and the precedent for splicing |
| `crates/axiom-host/src/{frame_sky,frame_depth_fog,frame_bloom}.rs` | CPU definitions the WGSL mirrors, pinned by `sky_shader_constants_match_the_host_definition` — **the parity-test precedent to copy** |
| `crates/axiom-field/src/ops/**` | the semantics being mirrored |

## Files owned

| Path | Action |
|---|---|
| `modules/axiom-gpu-backend/src/surface_program/emit.rs` | create — the emitter |
| `modules/axiom-gpu-backend/src/surface_program/emit_ops.rs` | create — `const EMIT: [EmitFn; 23]` |
| `modules/axiom-gpu-backend/src/surface_program/wgsl_template.rs` | create — the splice points |
| `modules/axiom-gpu-backend/src/scene_renderer.rs` | modify **minimally** — expose `SCENE_WGSL` splice markers only |
| `modules/axiom-gpu-backend/tests/surface_parity.rs` | create |

## Dependencies on earlier manifests

**`07`** (the plan, the parameter layout, the submodule directory). Parallel with
`10` and `11` **only if** `07` created `surface_program/` so the three do not
contend on `scene_renderer.rs`. If any of them must edit `scene_renderer.rs`
substantively, run them sequentially `08 → 11 → 10`.

## Public API / data contracts

### The emitter shape

```rust
type EmitFn = fn(&EmitCtx<'_>, node: NodeId) -> ();   // appends one `let n{i} = …;`
const EMIT: [EmitFn; 23] = [ /* FieldOp discriminant order */ ];
```

**One emitter function per operator, in a `const` table indexed by the opcode** —
the same discipline as the CPU evaluator's `OPS` table and `proc-texture`'s
dispatch. The two tables sit side by side and are read together; that adjacency
is what makes a semantic drift between them obvious in review.

Emission is a **flat forward pass in node id order**, producing SSA:

```wgsl
let n0 = in.object_pos;
let n1 = n0.y;
let n2 = 4.0;
let n3 = n1 * n2;
...
```

No recursion (`engine_no_recursion`), no expression-tree flattening, no temporary
naming scheme beyond `n{id}`. The field graph is *already* SSA — that is what an
id-ordered DAG with earlier-only inputs is — which is why this manifest is small.
That property was designed in at `01` precisely to make this stage trivial.

### Splicing into `SCENE_WGSL`

Follow `surface_encode.rs:74-76`'s precedent: concatenation, not a preprocessor.
`SCENE_WGSL` gains named splice markers, and the emitter produces:

```
[ SRGB_TRANSFER_WGSL ][ SCENE_WGSL_PREFIX ][ generated surface fn ][ SCENE_WGSL_SUFFIX ]
```

The generated function has a fixed signature and returns a fixed struct:

```wgsl
struct SurfaceOut {
    base_color: vec4<f32>, roughness: f32, metallic: f32,
    normal: vec3<f32>, emission: vec3<f32>, opacity: f32,
};
fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut { ... }
```

`fs` calls `axiom_surface` and feeds the existing lighting maths. **The lighting
model, the PCF, the ambient, the fog and the tonemap are untouched.** This is
"parameterise the fixed model by data", exactly as
`docs/engine-datafication.md:234-240` prescribes.

The default `axiom_surface` — for `surface_program == 0` — reproduces today's
behaviour exactly from the instance lanes, so **every existing app must be
pixel-identical**, and that is a test.

### Operator emission, and the two that need care

Twenty-one of the twenty-three ops emit one WGSL line each and are mechanical.
Two are not:

* **`Noise` / `Fbm`.** WGSL has no noise. `axiom-noise` keys
  `StableHash` (FNV-1a) by an integer lattice cell — an integer hash, which
  **is** expressible in WGSL with `u32` arithmetic. Emit the FNV-1a mix and the
  gradient lookup as WGSL helper functions generated once per shader, and pin them
  against `crates/axiom-noise/src/gradient_noise.rs` with a parity test. This is
  the single highest-risk item in the manifest: if the WGSL integer hash and the
  Rust one disagree by one bit, every noise-driven surface looks different on GPU
  than on CPU and than in every bake. **Write the parity test first.**
* **`Normalize`.** GPU `inversesqrt` is lower-precision than `1.0 / sqrt(x)`.
  Emit `v * (1.0 / length(v))` explicitly to match `03`'s stated order, and accept
  the small cost.

### The parity test — the manifest's real deliverable

```rust
// modules/axiom-gpu-backend/tests/surface_parity.rs
```

For a set of graphs covering every operator, evaluate on a sampled grid of
`EvalContext`s and compare:
* CPU: `FieldGraph::evaluate`.
* GPU: render the generated shader to a small offscreen target under the
  `offscreen` feature and read back.

Tolerance: **`1e-4` absolute on `0..=1` channels**, documented, never byte
equality (`03` explains why). A failure names the operator, not a WGSL line —
which is the brief's diagnostic requirement satisfied at the only place it can be.

Precedent to imitate: `sky_shader_constants_match_the_host_definition` and
`capability_bits_are_the_gpu_shader_contract` already pin CPU↔WGSL agreement in
this module. Extend the same idea, do not invent a new mechanism.

**Note the feature gate:** without `--features offscreen` the GPU arm silently
falls back, so a parity test that does not assert it actually ran on the GPU is
worthless. Assert the backend kind.

## Explicitly excluded

* **No caching.** Every call regenerates the string. `09` adds the cache.
* **No vertex-stage emission.** `Displacement` is `10`.
* **No lighting-model branching in the generated code.** `11`.
* **No shader minification, no dead-code elimination on the WGSL.** The semantic
  graph was already const-folded, CSE'd and DCE'd by `02`; doing it again on text
  is the ceremonial second optimiser `00-architecture-findings.md` §3C rejects.
* **No `#ifdef`-style preprocessor.** Concatenation only.
* **No raw shader escape hatch** — its placement is decided in `07`; implement it
  in a *separate* change after `09`, so it can never be confused with the
  generated path.

## Determinism requirements

* Same `Surface` → byte-identical WGSL string. This is what makes the string
  cacheable by digest in `09`.
* Emission order is node id order; no map iteration anywhere in the emitter.

## Serialization requirements

The generated WGSL is **not** serialized or committed. Its determinism is proven
by a test asserting string equality across two generations of the same surface,
not by a golden `.wgsl` file — the repo tracks **no** `.wgsl`/`.glsl` files and
this manifest must not start.

## Testing requirements (100%)

* One test per emitter function — 23 minimum, same as the CPU table.
* Generated string determinism (two generations, byte-equal).
* `surface_program == 0` produces a shader that renders **pixel-identically** to
  today for every existing app — assert against the existing burnt-rubber goldens.
* The noise parity test, written first and named prominently.
* The full parity sweep across every operator at the stated tolerance.
* A generated shader actually compiles: `create_shader_module` succeeds for every
  test surface. A WGSL syntax error must fail a test, not a frame.
* Compilation failure is surfaced as a structured error naming the surface's
  digest and the offending channel — never a panic, never a silent black draw.

## Architecture tests

`cargo xtask check-architecture`; hygiene (see the `canvas` trap above);
`engine_no_large_files` — `scene_renderer.rs` is already 2650 lines against a
1000-line lint. **It is in the baseline at 0**, meaning any *new* file over the
cap trips the gate. Put the emitter in `surface_program/` and keep each file
small; do not grow `scene_renderer.rs`.

## Performance risks

* **Generated shader size.** A 256-node graph emits 256 WGSL lines. On the WebGL2
  path, `wgpu` cross-compiles WGSL→GLSL at pipeline creation, so a large shader is
  a large *compile*, not a large *frame*. `09` moves that compile to the
  preparation barrier, which is the whole mitigation. Until `09` lands, generation
  in a frame path is forbidden — assert it by only calling the emitter from bind.
* **Instruction count in the fragment stage.** The existing shader already
  evaluates *both* arms of every capability `select()`. A 100-node surface graph
  on top of that is a real per-pixel cost on a phone. Record the node count in the
  submission report so a heavy surface is visible in telemetry rather than only in
  the frame time.
* **Do not add vertex attributes.** 16 of 16 are used; a 17th fails pipeline
  creation on the browser fallback path (`scene_renderer.rs:245-252`).

## Migration considerations

None for existing content — `surface_program == 0` is the untouched path, and the
pixel-identity test is the proof.

## Completion criteria

1. A `Surface` emits deterministic, compiling WGSL.
2. CPU↔GPU parity holds for every operator at the documented tolerance, with the
   noise hash pinned by its own test.
3. Every existing app is pixel-identical.
4. Compilation failure produces a structured, surface-naming error.
5. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint count
   rises.

## Validation commands

```sh
cargo test -p axiom-gpu-backend --features offscreen
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
cargo run -p axiom-shot --features offscreen -- \
  --app burnt-rubber-straight --backend gpu --tick 0 --out screenshots/br-gpu.png
```

## Parallel safety

**Wave 8.** Parallel with `10` and `11` **only** if all three confine themselves
to disjoint files under `surface_program/`. Otherwise sequential.

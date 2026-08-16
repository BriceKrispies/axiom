# 03 — The field CPU evaluator (the semantic reference implementation)

## Objective

Land the pointwise evaluator: given a validated `FieldGraph`, a `FieldParams` and
an `EvalContext`, produce a `FieldValue`. This evaluator **defines what the
language means**. Every other realisation — the WGSL emitter in `08`, the
Canvas2D shading path in `07` — is a mirror checked against it.

## Architectural placement

**Layer: `field`** (`crates/axiom-field`).

### Why the CPU evaluator is the reference, on evidence rather than preference

The repository already practises exactly this pattern. `crates/axiom-host` holds
the authoritative CPU definitions — `FrameSky::radiance`,
`FrameDepthFog::mix_fraction`, `FrameBloom::tonemap`, `apply_frame_retro_32bit`,
`apply_frame_postprocess` — and `modules/axiom-gpu-backend`'s WGSL mirrors them,
pinned by parity tests such as `sky_shader_constants_match_the_host_definition`
and `capability_bits_are_the_gpu_shader_contract`.

There is also a second, harder reason. `modules/axiom-canvas2d-backend` **cannot
execute a shader** — it flat-shades per triangle and discards uv and normals at
upload (`mesh_cache.rs:27-31`). Without a CPU evaluator, every field-authored
surface would be a *dropped* capability on that backend. With one, it is a
*correct degrade*: the backend samples the surface's channels at each triangle's
centroid through this evaluator. The CPU evaluator is therefore not a testing
convenience — it is a shipping render path (`07`).

## Existing code involved

| Path | Why |
|---|---|
| `crates/axiom-field/src/**` | from `01` and `02` |
| `crates/axiom-proc-texture/src/dispatch.rs` | **the template**, all 35 lines of it |
| `crates/axiom-proc-core/src/proc_core.rs:31-66` | the `try_fold`-over-a-cache idiom — copy the shape, **not** the `F: Fn` parameter |
| `crates/axiom-noise/src/{gradient_noise,fbm}.rs` | `value_noise`, `Fbm`, `FbmConfig` |
| `crates/axiom-mesh-ops/src/implicit_surface.rs:22-32` | the existing central-difference gradient convention |

## Files owned

| Path | Action |
|---|---|
| `crates/axiom-field/src/eval.rs` | create — the fold + register file |
| `crates/axiom-field/src/dispatch.rs` | create — `const OPS: [FieldOpFn; 23]` |
| `crates/axiom-field/src/ops/` | create — one file per operator family (`arith.rs`, `shape.rs`, `vector.rs`, `spatial.rs`, `source.rs`) |
| `crates/axiom-field/src/field_graph.rs` | modify — add `evaluate` |
| `crates/axiom-field/src/canonical.rs` | modify — enable `Noise`/`Fbm` constant folding now that semantics exist |
| `crates/axiom-field/src/lib.rs`, `layer.toml`, `ARCHITECTURE.md` | modify |

## Dependencies on earlier manifests

**`01` and `02`, strictly.** Same crate, same `lib.rs`.

## Public API / data contracts

```rust
impl FieldGraph {
    pub fn evaluate(&self, ctx: &EvalContext) -> FieldResult<FieldValue>;
    pub fn evaluate_at(&self, ctx: &EvalContext, node: NodeId) -> FieldResult<FieldValue>;
}
```

### The evaluator shape — mandated, not suggested

```rust
type FieldOpFn = fn(&FieldEvalStep<'_>) -> FieldValue;
const OPS: [FieldOpFn; 23] = [ /* discriminant order */ ];
```

* **A `const` fn-pointer table indexed by `op as usize`.** Not a `match`
  (`engine_no_branching`), and not a generic closure parameter
  (`engine_no_retained_state` bans `F: Fn(..)`, which is why `ProcCore::execute`
  cannot be reused here even setting performance aside).
* **A flat `try_fold` in id order over a register file**, not a recursive walk
  (`engine_no_recursion`). Inputs are always already computed because ids are
  strictly increasing.
* **A fixed-size register array**, not a `Vec` grown per call. `MAX_NODES = 256`
  and `FieldValue` is 5 words, so a `[FieldValue; 256]` register file is ~5 KB on
  the stack and allocates nothing. **This is the reason `field` does not build on
  `proc-core`:** `ProcCore` allocates a `Vec<Out>` *and* mints an
  `EntropyStream` per node per call, which is fine once per artifact and
  catastrophic once per texel.
* **Operators are total.** Every operator returns a `FieldValue` — no `Option`,
  no error. All rejection happened in `02`; an evaluator that can fail at a point
  would need a per-sample error path in the innermost loop of every bake.
  Out-of-range accesses are made unrepresentable by validation, and the register
  file is pre-filled with a documented zero default.

### Operator semantics — the contract `08` must mirror

Document each in one line in the code, because the WGSL emitter is written
against these words:

| Op | Semantics |
|---|---|
| `Const` | the parameter words, typed by the declared `FieldType` |
| `Point` / `Uv` / `Normal` / `Time` | the corresponding `EvalContext` field |
| `Param` | `FieldParams[slot]` |
| `Add`/`Sub`/`Mul`/`Min`/`Max` | component-wise; a `Scalar` input broadcasts |
| `Abs` | component-wise `f32::abs` |
| `Clamp(x, lo, hi)` | component-wise; `lo > hi` yields `lo` (documented, not UB) |
| `Mix(a, b, t)` | `a + (b - a) * t`, component-wise, **`t` unclamped** — this exact form, because `a*(1-t)+b*t` differs in the last bit |
| `Smoothstep(e0, e1, x)` | `t = clamp((x-e0)/(e1-e0), 0, 1); t*t*(3-2*t)`; `e0 == e1` yields `0` |
| `Dot` | scalar dot product over the inputs' common width |
| `Length` | `sqrt(dot(v, v))` |
| `Normalize` | `v / length(v)`; **length below `Epsilon` yields `+Y`**, matching `implicit_surface.rs`'s existing deterministic default |
| `Compose(width)` | assemble a vector from `width` scalar inputs |
| `Component(i)` | extract lane `i` as a `Scalar` |
| `Noise(seed)` | `axiom_noise::value_noise(seed, point_input)` — a `Scalar` |
| `Fbm(seed, cfg…)` | `axiom_noise::Fbm` with octaves/lacunarity/gain/warp from parameter words |
| `Transform` | `Mat4` from parameters × the `Vec3` input, as a point (w = 1) |

### Floating-point determinism, stated precisely

* **CPU-to-CPU determinism is exact and required.** Same graph, same context →
  bit-identical `f32` on every target including wasm32. This holds because the
  algebra excludes transcendentals and division, and because `sqrt` is
  IEEE-754-exact. `Normalize` is the only reciprocal and is defined as
  `v * (1.0 / len)` — fix the order and write it down.
* **CPU-to-GPU parity is a tolerance, not an equality.** GPUs are permitted
  wider intermediates and a lower-precision `inversesqrt`. `08` pins parity with
  a sampled-grid test at a documented tolerance (start at `1e-4` absolute on
  `0..=1` channels) — never with byte equality.
* Note for the record: `engine_no_unportable_float` exists in `tools/lints/` but
  is **not registered** in `[workspace.metadata.dylint].libraries`, so float
  portability is currently unenforced by the gate. Do not rely on it.

### Capability annotations — not yet, and here is the line

Every one of the 23 operators is implementable on the CPU and in WGSL. **No
operator needs a capability annotation today**, which is the strongest argument
that the algebra is correctly sized. The moment an operator is proposed that one
backend cannot express (a texture sample, a screen-space derivative), it needs a
`RenderCapability` bit and a validation step — and that is the signal to reject
the operator rather than to add the annotation. Record this rule in
`ARCHITECTURE.md`.

## Explicitly excluded

* No SIMD, no batched multi-point evaluation, no caching of sub-results across
  calls. `05` may add a batched entry point later if measurement demands it;
  designing for it now would leak into the public API.
* No GPU concepts of any kind.
* No `evaluate` that takes a callback for external inputs — that is the banned
  `impl Fn` shape (`crates/axiom-mesh-ops/src/implicit_surface.rs:10-14`).

## Determinism requirements

Restated as a test obligation: a golden table of `(graph, context) → FieldValue`
committed as bytes, covering every operator, asserted bit-exactly.

## Serialization requirements

None new. `FieldValue` gains `write_to`/`read_from` only if `12` needs it for
introspection output; do not add it speculatively.

## Testing requirements (100%)

* **One test per operator function** — 23 minimum. The `const` table means each
  is a separate function and therefore a separate coverage region.
* An out-of-range opcode cannot occur post-validation, but `dispatch` must still
  have the `OPS.get(index)` guard and a test for it, exactly as
  `crates/axiom-proc-texture/src/dispatch.rs` does.
* Documented-edge tests: `Clamp` with `lo > hi`; `Smoothstep` with `e0 == e1`;
  `Normalize` of the zero vector; `Mix` with `t` outside `0..=1`;
  `Component` on every lane of every width.
* Scalar broadcast across every width-generic op.
* Determinism: evaluate twice, assert bit-equal; evaluate a serialized/
  deserialized copy, assert bit-equal.
* A composed end-to-end test that is not a toy — the reference case for the whole
  design: **a spatial gradient mixed with fbm driving a scalar**, i.e.
  `Mix(Const(a), Const(b), Smoothstep(0, 1, Add(Mul(Component(Point,1), k), Mul(Fbm(seed, Point), w))))`,
  asserted against a committed golden.

## Architecture tests

`engine_no_recursion` at 0, `engine_no_branching` at 0, `engine_no_retained_state`
contributing nothing new from this crate.

## Performance risks

This is the hot path of every bake — it runs once per texel, per lattice node,
per vertex. Concretely: a 128×128 asphalt texture is 16,384 evaluations of a
~12-node graph.

* **Register file on the stack, zero allocation per call** — non-negotiable.
* `FieldValue` must stay `Copy` and small (5 words). Do not add a field to it.
* The per-call cost is `O(nodes)`, not `O(nodes × inputs)` — inputs are read by
  index from the register file, never cloned. This is the specific defect
  `ProcCore` has (`cache[id].clone()` per edge) and the reason it is not reused.
* Measure before optimising. The bar to clear is the status quo: burnt-rubber's
  hand-written `asphalt_albedo()` generates 128×128 in a preparation task today,
  and the field version must not be meaningfully slower **at preparation time**,
  where there is no frame budget at all.

## Migration considerations

None. Additive.

## Completion criteria

1. `FieldGraph::evaluate` exists and is total post-validation.
2. All 23 operators implemented, documented, and individually tested.
3. Committed golden covering every operator, asserted bit-exactly.
4. The composed gradient×fbm reference case passes against a golden.
5. Zero allocation per `evaluate` call (assert by construction — the register
   file is an array; state it in a comment and in `ARCHITECTURE.md`).
6. `scripts/coverage.sh` 100/100/100; `cargo xtask check-architecture` exits 0;
   no dylint count rises.

## Validation commands

```sh
cargo test -p axiom-field
cargo xtask check-architecture
cargo test --workspace
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 4, width 1.** Owns `crates/axiom-field/src/lib.rs`. Sequential after `02`.
Unblocks `04` and `05`, which may then run in parallel.

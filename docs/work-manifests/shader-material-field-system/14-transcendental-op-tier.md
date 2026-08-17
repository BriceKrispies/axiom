# 14 — The transcendental operator tier

## Objective

Give the field algebra `Sin`, `Cos`, `Pow` and `Exp`, so a GPU-first procedural
vocabulary is expressible directly rather than through workarounds. Carry them at
a **wider, documented CPU↔GPU tolerance** than the rest of the algebra, and get
past the 24-variant enum cap by making `FieldOp` a `u16` newtype with associated
constants — the shape `axiom-recipe` and `RenderPipelineKind` already use.

## Why this exists

Manifest 01 excluded transcendentals on the grounds that they "differ between CPU
`f32` and GPU `f32` by more than the parity tolerance". That reasoning was sound
but the conclusion was too strong: it capped the algebra to protect a *single*
global tolerance, when the honest answer is a per-operator tolerance.

The cost is already visible in shipped code:

* **Manifest 10 could not express a true twist.** It routes through a per-frame
  `Mat4` parameter the app computes, documented in `emit_vertex.rs` as a gap.
* **Manifest 10's `ripple` uses a backwards `Smoothstep` on a radial distance**
  rather than a sine — a workaround its own report calls "the trick that makes
  trigonometry unnecessary".
* **Manifest 05 listed `exp` as one of four reasons the MetaSurface rewrite was
  refused** — `skin_of` weights are `exp(-capsule_sdf/k)` per capsule and the
  algebra had no `exp`. This manifest removes one of those four blockers (the
  other three — per-capsule identity, the node budget, and the differing output
  contract — stand, so MetaSurface is still not rewritten).
* The classic procedural vocabulary the shader-crucible app (manifest 15) must
  demonstrate — marble veining, wood grain, water — is built on `sin` and `pow`.

**This is not a Canvas2D concession being lifted.** Canvas2D already drops 8 of
13 capabilities and substitutes `ProceduralSurface` with a reported per-triangle
degrade; it constrains nothing here. The constraint being relaxed is the
CPU-reference parity budget, which exists for the bake path and the software arm
alike.

## Architectural placement

**Layer: `field`** (`crates/axiom-field`) for the operators; **Module:
`gpu-backend`** (`modules/axiom-gpu-backend/src/surface_program/`) for their
emission and parity. No new package, no new layer, no law change.

## Existing code involved

| Path | Role |
|---|---|
| `crates/axiom-field/src/field_op.rs` | `FieldOp`, `#[repr(u16)]`, 23 variants, `FIELD_OP_COUNT`, `ALL`, `code`, `from_code` |
| `crates/axiom-field/src/signature.rs` | `const SIGNATURES: [FieldSignature; 23]` |
| `crates/axiom-field/src/dispatch.rs` | `const OPS: [FieldOpFn; 23]` |
| `crates/axiom-field/src/ops/**` | the operator bodies |
| `crates/axiom-field/src/const_fold.rs` | `FOLDABLE: [bool; 23]`, delegates to `dispatch::field_eval` |
| `crates/axiom-field/src/type_check.rs` | the forward-fold checker |
| `modules/axiom-gpu-backend/src/surface_program/emit_ops.rs` | `const EMIT: [EmitFn; 23]` |
| `modules/axiom-gpu-backend/src/surface_program/parity.rs` | the sweep + `TOLERANCE` |
| `modules/axiom-render/src/render_pipeline_kind.rs` | **the precedent**: a marker struct with `pub const` `u32` associated constants, deliberately not an enum |
| `crates/axiom-recipe/src/node.rs` | `Node.op` is already a bare `u16`; the op code space has always been flat |

## Files likely to change

* `crates/axiom-field/src/field_op.rs` — the newtype conversion
* `crates/axiom-field/src/{signature,dispatch,const_fold,type_check}.rs` — table widths
* `crates/axiom-field/src/ops/transcendental.rs` — **create**
* `crates/axiom-field/{layer.toml, ARCHITECTURE.md}`
* `modules/axiom-gpu-backend/src/surface_program/{emit_ops,parity}.rs`
* `crates/axiom-field/tests/eval_golden.rs` — four new golden rows

## Dependencies

**`13`.** Land the vertical slice against the settled 23-op algebra first, so the
slice proves the architecture rather than proving a brand-new operator tier. This
manifest then widens the vocabulary the crucible app (15) demonstrates.

Nothing else depends on it. It is additive.

## Public API / data contracts

### `FieldOp` becomes a `u16` newtype

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldOp(u16);

impl FieldOp {
    pub const CONST: FieldOp = FieldOp(0);
    // … 0..=22 unchanged, byte-for-byte …
    pub const SIN:   FieldOp = FieldOp(23);
    pub const COS:   FieldOp = FieldOp(24);
    pub const POW:   FieldOp = FieldOp(25);
    pub const EXP:   FieldOp = FieldOp(26);

    pub const fn code(self) -> u16;
    pub fn from_code(code: u16) -> Option<FieldOp>;
}
pub const FIELD_OP_COUNT: usize = 27;
```

**Why a newtype and not a second enum.** `engine_no_large_enums` counts
`enum_def.variants.len()` and the prescribed fix is nested sub-enums — but that
resets the count *per level* and the dispatch technique needs one flat
discriminant space indexing one `const` table. A newtype with associated
constants is not an enum, so the lint does not apply, the discriminant stays the
table index, and the shape already has two precedents in this repo
(`RenderPipelineKind::{BASIC_LIT, UNLIT}` as `u32` consts; `Node.op` as a bare
`u16`).

**The wire format does not change.** `Node.op` was always a `u16` and codes 0–22
keep their values, so **every existing serialized graph, every committed golden
and every digest stays valid.** That is a hard requirement of this manifest, and
it is the reason the new codes are appended rather than inserted.

**What is lost:** exhaustive `match` over operators. Nothing in the spine matches
on `FieldOp` — dispatch is table-indexed by construction — so this costs nothing
real. Tests that enumerate operators use `FieldOp::ALL`.

### The four operators

| Op | Inputs | Params | Output | Semantics |
|---|---|---|---|---|
| `Sin` | 1 | 0 | width-generic | per lane `f32::sin` |
| `Cos` | 1 | 0 | width-generic | per lane `f32::cos` |
| `Pow` | 2 | 0 | width-generic | per lane `f32::powf(a, b)`; `a < 0` with non-integral `b` yields `0.0`, documented, not NaN |
| `Exp` | 1 | 0 | width-generic | per lane `f32::exp` |

> **CORRECTION — that `Pow` rule is necessary but not sufficient.** WGSL's
> `pow(e1, e2)` is undefined for `e1 < 0` **including integral exponents**, and
> for `e1 == 0` with `e2 <= 0`. A CPU rule returning `-8.0` for `Pow(-2, 3)`
> would therefore be unmirrorable — a silent CPU/GPU divergence. The shipped rule
> covers all three hazards in one statement: **`Pow(a, b) = f32::powf(a, b)`
> where `a > 0`, and `0.0` everywhere else.** Total, never `NaN`, never an
> infinity from a finite base, and it mirrors exactly as
> `select(vec4(0.0), pow(max(a, 0.0), b), a > 0.0)` — the builtin is never asked
> for a value it lacks. Documented consequence: a square is `Mul(x, x)`, not
> `Pow(x, 2)`.

`Div` is still **excluded** — division by zero remains a NaN source and
`Pow(x, -1)` covers reciprocal with a documented zero behaviour.

`Log`, `Atan2`, `Sqrt` are **not** added. `Sqrt` is already reachable as
`Pow(x, 0.5)` and `Length`; the other two have no consumer yet, and the operator
admission test in `12-agentic-introspection-and-serialization.md` requires two
unrelated ones.

### The per-operator tolerance — the substantive change

`parity.rs` currently carries one `TOLERANCE = 1e-4`. It gains a per-operator
table:

```rust
const TRANSCENDENTAL_TOLERANCE: f32 = 1e-3;   // Sin, Cos, Pow, Exp
const TOLERANCE: f32 = 1e-4;                  // everything else, unchanged
```

> **OUTCOME — the premise was wrong, and the finding inverts it.** Measured on a
> discrete Vulkan adapter over 24 sampled contexts: `Sin` 5.07e-7, `Cos` 4.18e-7,
> `Exp` 2.39e-7, `Pow` 2.29e-5 (at output magnitude 103.8 — i.e. 2.2e-7
> *relative*). All four agree to roughly **1e-6 relative, about ten f32 ulps**.
>
> **The transcendental tier did not need a tolerance wider than `1e-4`. It needed
> a tighter one.** Both declared constants — `1e-6` for `Sin`/`Cos`/`Exp` and
> `3e-5` for `Pow`, split because `Pow`'s larger absolute delta is magnitude and
> not inaccuracy — sit *below* the exact tier's `1e-4`. The `1e-3` starting bound
> below would have been 40× looser than the hardware needs.
>
> That means **manifest 01's original justification for excluding these operators
> was false.** They were excluded on the belief that they "differ between CPU and
> GPU `f32` by more than the parity tolerance"; they do not. The algebra was
> capped for a hazard that measurement does not support. The real constraints on
> `Pow` turned out to be *definedness*, not precision (see below).

**Requirements on this number.** `1e-3` is a starting bound, not a finding. The
parity test must **record the measured worst-case delta per operator** in its
output and the implementer must put the real numbers in `ARCHITECTURE.md`. If the
measured worst case is materially below `1e-3`, tighten the constant to just
above it — a tolerance looser than the hardware needs is a tolerance that hides a
future regression.

**CPU-to-CPU determinism is unchanged and still exact.** `f32::sin`/`cos`/`exp`/
`powf` are deterministic for a given input on a given target. The widened budget
covers CPU↔GPU only. Note that Rust's `sin` is not guaranteed bit-identical
*across targets* — record this in `ARCHITECTURE.md` as a known limit of the
transcendental tier specifically, and keep the existing bit-exact golden
assertions for the other 23 operators rather than weakening them globally.

### Constant folding

The four are foldable (`FOLDABLE[code] = true`) — they are pure and total. Note
the consequence: a folded `Sin` is computed on the CPU and baked into a `Const`,
so a graph that folds is *more* CPU-exact than one that doesn't. That is correct
and worth a sentence in `ARCHITECTURE.md`.

## Explicitly excluded

* No `Div`, `Log`, `Atan2`, `Sqrt`, `Fract`, `Mod`, `Step`, `Cross`.
* No change to the other 23 operators' semantics, codes, or tolerance.
* **No change to the wire format or to any existing digest.**
* No MetaSurface rewrite — `Exp` removes one of the four blockers in manifest 05;
  three remain.
* No texture sampling, no screen-space derivatives.
* No relaxation of the Canvas2D degrade — it evaluates these on the CPU like
  every other operator.

## Determinism requirements

* Codes 0–22 unchanged; every existing golden and digest must still pass
  untouched. **This is the first thing to verify and the last thing to re-verify.**
* New golden rows for the four operators in `tests/eval_golden.rs`, bit-exact on
  CPU.
* `canonicalize` remains idempotent and order-independent.

## Testing requirements (100%)

* One test per new operator function, plus width-generic broadcast on each.
* Documented-edge tests: `Pow` with a negative base and non-integral exponent;
  `Pow(x, 0)`; `Exp` overflow to `inf` and how `NonFiniteConstant` treats it in
  folding; `Sin`/`Cos` at large arguments.
* `FieldOp::from_code` rejects 27 and above; `ALL.len() == 27`.
* **A test asserting codes 0–22 are unchanged** — the wire-compatibility pin.
* Parity sweep extended to 27 operators, transcendentals at the wider tolerance,
  with the measured worst case printed.
* The existing 23 operators must still pass at `1e-4` — assert the tolerance
  table does not accidentally widen them.

## Architecture tests

`cargo xtask check-architecture`; `engine_no_large_enums` must report **zero**
for `axiom-field` (the newtype is not an enum); `engine_no_branching` and
`engine_no_recursion` stay at 0.

## Performance risks

* Transcendentals are genuinely more expensive per texel than the existing
  algebra. A bake that uses them costs more at preparation time; there is no
  frame budget there, but measure it.
* On the GPU they compile to hardware instructions and are cheap. No pipeline or
  variant consequence — the op set is still closed and still lowered at the
  preparation barrier.
* `FieldOp::ALL` grows from 23 to 27; every `const` table widens by four rows.
  Nothing iterates them per frame.

## Migration considerations

None externally. Codes 0–22 are frozen, so no serialized graph, golden or digest
moves. Internally, every `[T; 23]` becomes `[T; 27]` and the compiler finds them
all.

## Completion criteria

1. `FieldOp` is a `u16` newtype with associated constants; `engine_no_large_enums`
   reports zero for the crate.
2. `Sin`, `Cos`, `Pow`, `Exp` implemented, folded, type-checked, emitted.
3. **Codes 0–22 unchanged and every pre-existing golden and digest passes
   untouched.**
4. Parity: 23 operators at `1e-4`, 4 at the transcendental tolerance, with the
   measured worst case recorded in `ARCHITECTURE.md`.
5. A `sin`-based pattern (marble veining or water) demonstrated as an authored
   graph in a test, with no new Rust beyond these four operators.
6. Coverage 100/100/100 on `axiom-field` and `axiom-gpu-backend`;
   `check-architecture` exits 0; `check-slices` passes; no dylint count rises.

## Validation commands

```sh
cargo test -p axiom-field -j 4
cargo test -p axiom-gpu-backend --features offscreen -j 4
cargo test --workspace -j 4 --no-fail-fast
cargo xtask check-architecture
cargo run -p xtask -- check-slices
cargo llvm-cov clean --workspace && cargo +nightly llvm-cov --branch -p axiom-field -p axiom-gpu-backend
cargo dylint --all -- -p axiom-field -p axiom-gpu-backend --all-targets
```

## Parallel safety

**Sequential, after `13`.** Owns `crates/axiom-field/src/lib.rs` and the
`surface_program/` emitter tables. Blocks `15` (the shader-crucible app), which
should demonstrate the full 27-operator vocabulary.

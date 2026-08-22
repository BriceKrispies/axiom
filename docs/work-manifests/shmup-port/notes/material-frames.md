# `material_shader/frames` — the projection frames

Source: `C:/dev/Claude-of-Duty/src/materials/shader.js`, `PARS_FRAGMENT`,
lines 171-208 — `owTangentFrame`, `struct OwFrame`, `owAxisFrame`,
`owOrthonormalise`.

Target: `modules/axiom-gpu-backend/src/material_shader/frames.rs`.

This is the layer every other one is written against: triplanar builds three of
these and blends across them, POM marches in the frame's tangent space,
de-tiling and the detail layer sample through its `uv`, and the normal blend
resolves `nT` through `T`/`B`/`N`. So it is transcribed at the level of the
individual cross product, and both cross products are asserted on the *text* of
the WGSL as well as on its behaviour.

## The WGSL surface siblings compose against

```wgsl
struct OwFrame { uv: vec2<f32>, T: vec3<f32>, B: vec3<f32>, N: vec3<f32> }

fn owAxisFrame(p: vec3<f32>, n: vec3<f32>, axis: i32, owTile: vec4<f32>) -> OwFrame
fn owOrthonormalise(f: ptr<function, OwFrame>, n: vec3<f32>)
fn owTangentFrame(dEyeDx: vec3<f32>, dEyeDy: vec3<f32>,
                  dUvDx: vec2<f32>, dUvDy: vec2<f32>, n: vec3<f32>) -> mat3x3<f32>
fn owTangentFrameScreen(eye: vec3<f32>, n: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32>
```

Names are the source's own, verbatim, camelCase and all: twelve layers are being
transcribed from the same GLSL at once, and a sibling reaching for `owAxisFrame`
should find `owAxisFrame`. The `ow` prefix also cannot collide with the
`axiom_*` prelude the composer splices this next to.

Two departures from the source's signatures, both forced and both documented at
the call site:

* **`owTile` is a parameter.** The source reads the `owTile` uniform
  (`xy` = tiles-per-metre scale, `zw` = offset) from inside `owAxisFrame`. The
  layer calling convention forbids reading a global, so it is the fourth
  argument. The composer supplies it from the packed parameter block.
* **`owTangentFrame` takes its four derivatives.** See below.

`owOrthonormalise` keeps GLSL's `inout` as `ptr<function, OwFrame>`, so the call
shape is the source's: `var f = owAxisFrame(...); owOrthonormalise(&f, owNp);`.

## The CPU reference

`OwFrame`, `OwTangentBasis`, `ow_axis_frame`, `ow_orthonormalise`,
`ow_tangent_frame`, plus four written-out GLSL builtins (`gl_step`, `gl_mix`,
`gl_normalize`, `gl_inverse_sqrt`). Branchless: the `if / else if / else` axis
chain is an index, `usize::from(axis != 0) * (1 + usize::from(axis != 1))`,
which reproduces the chain exactly — including that **anything that is not 0 or
1, negative values included, takes the Z arm**.

`OwTangentBasis` is what GLSL's `mat3(T * sc, B * sc, n)` is: three *columns*,
which the source's one call site immediately unpacks as `tbnV[0..2]`.
`axiom_math::Mat3` is not used — it is documented as the 2D affine transform
primitive and is the wrong noun for a TBN basis.

## What was nearly got wrong, by name

**`step`, not `sign`.** The per-axis sign is
`mix( vec3(-1.0), vec3(1.0), step( 0.0, n ) )`. Three functions disagree at
zero: GLSL `sign(0.0)` is `0.0` (which would collapse the basis to the zero
vector), Rust's `f32::signum` is `+1.0` at `+0.0` and `-1.0` at `-0.0`, and
`step(0.0, x)` is `1.0` at **both** zeroes. Every axis-aligned box face has two
exactly-zero normal components, so this is not a corner case, it is most of a
building. `gl_step` is written out rather than reached for, and
`a_zero_normal_component_selects_the_positive_axis` pins `-0.0` specifically.

**Handedness.** A basis with the cross-product operands swapped is still
orthonormal, still compiles, and mirrors every normal map. The two cross
products in `owTangentFrame` take their operands in *opposite* orders
(`cross(q1, n)` then `cross(n, q0)`) and `owOrthonormalise` computes
`B = cross(N, T)`. All three are asserted on the WGSL text itself, because the
transcription — not the maths — is the risk here.

The three static axis bases are right-handed (`cross(T, B) == N`) for all six
combinations of axis and normal sign; a swapped pair in any arm flips one of
those six and `the_axis_bases_are_right_handed_for_every_axis_and_sign` fails.

`owTangentFrame`'s handedness, by contrast, is **not** a fixed sign — it
reproduces whatever handedness the mesh's uv winding has, and a test says so
outright. That is correct behaviour (a mirrored uv island should shade
mirrored), and it is worth knowing before someone "fixes" a `-1` they see in a
debug view.

**The orthonormalisation order.** `f.B = cross( n, f.T )` runs *after*
`f.T = normalize( f.T - n * dot( n, f.T ) )`, so `B` is built from the
projected, renormalised tangent. Reordering leaves `B` non-perpendicular to the
new `T` whenever `n` is off-axis, and the resulting frame is not orthonormal at
all. `orthonormalising_uses_the_projected_tangent_not_the_original` computes
what the wrong order would give and asserts against it. The source's first line,
`f.N = n`, is inert — nothing after it in that function reads `f.N` — and is
transcribed anyway.

**No reciprocal-multiply.** `gl_normalize` is a component-wise **division** by
`length(v)`, not `v * (1 / length(v))`; `owTangentFrame`'s `sc`, on the other
hand, *is* an `inversesqrt`-and-multiply, because that is what the source
writes. Neither was tidied into the other.
`axiom_math::Vec3::normalize` is also not used: it rejects the zero vector with
a `MathError` where GLSL propagates a NaN, and this is a transcription of GLSL.

## `dpdx`/`dpdy`: a GPU-only input, made explicit — and still pinned

`owTangentFrame` reads `dFdx(eye)`, `dFdy(eye)`, `dFdx(uv)`, `dFdy(uv)`. A
screen-space derivative has no CPU equivalent: there is no neighbouring pixel.
So the arithmetic takes the four derivatives as parameters on both sides, and
the source's own three-argument signature survives as `owTangentFrameScreen`, a
fragment-only wrapper that supplies them. This is the shape
`apps/shmup/src/sky/dome.rs` already established for `fwidth`.

The wrapper is not left unverified. The parity test drives it over a probe whose
`eye` and `uv` are linear in `position.xy` with **dyadic** coefficients
(`0.25`, `0.125`, `0.03125`, …), so every product and sum is exactly
representable in `f32` and `f(x+1) - f(x)` is exact. The hardware's derivative
is therefore a known constant *whatever quad it picks and whether it computes
coarse or fine*, and that constant is what the CPU side is fed. A wrapper that
swapped `dpdx` for `dpdy` fails, and the test also asserts that the swapped
frame really is a different frame, so the pin is not vacuous.

`owTangentFrameScreen` is the only fragment-stage-only item in `FRAMES_WGSL`.

## CPU↔GPU parity: what was measured

Four entry points on a real adapter (Vulkan), 24 samples x 3 output rows each,
into an `Rgba32Float` target. The probe set sweeps all three axis arms plus the
out-of-range arms `7` and `-1`, both normal signs, exactly-zero and `-0.0`
normal components, negative positions, non-trivial tiling, and — for the tangent
frame — a degenerate patch with both uv derivatives zero, so the source's
`det == 0.0` ternary actually fires and is asserted to give a zero `T` and `B`
rather than a NaN.

The tolerance is **two measured parts**, not one number:

```
budget(v) = 1.0e-7 + 1.2e-7 * |v|
```

One absolute number would have been dishonest: the lanes range from an
exactly-`±1.0` basis component to a tiled uv in the tens, and a budget wide
enough for the second is hundreds of ULPs of the first. The relative term is
one `f32` ULP (`2^-23 = 1.19e-7`), because a GPU may contract
`uv * owTile.xy + owTile.zw` into a single-rounding `fma` where the CPU rounds
the multiply and the add separately. The floor covers the basis lanes, whose
only divergence is `normalize` evaluated as an `rsqrt`.

| entry point | worst absolute delta | share of budget used |
|---|---|---|
| `owAxisFrame` | `4.77e-7` | 0.631 |
| `owOrthonormalise` | `4.77e-7` | 0.631 |
| `owTangentFrame` | `1.19e-7` | 0.678 |
| `owTangentFrameScreen` | `1.19e-7` | 0.661 |

`4.77e-7` is `2^-21`, exactly one ULP at the magnitude of the uv it occurred on
(~5.5); `1.19e-7` is `2^-23`, exactly one ULP in `[1, 2)`. Every divergence in
this layer is one ULP of `fma` contraction and nothing else. The budget is
~1.5x what this hardware needs — headroom for another adapter's rounding, and
nowhere near the 10x that would make it a rubber stamp. `report` prints both
numbers under `--nocapture`, so the table above is reproducible rather than
remembered.

Two things are asserted **bit-equal** rather than within tolerance, because they
are selects over literals with no arithmetic at all: rows 1 and 2 of every
`owAxisFrame` sample (the `T.z`/`B`/`N` lanes). If those ever drift, something
structural is wrong, not something numeric.

`assert_varies` guards against the vacuous pass: a frame that read as a constant
would match a constant CPU side and prove nothing.

## Things the orchestrator needs to know

1. **The dominant-axis selection is *not* in this layer.** `owAxisFrame` takes
   `axis` as an argument; `MAIN_FRAGMENT`'s
   `int axis = ( abs(owNp.x) > abs(owNp.y) ) ? ( ... ) : ( ... )` belongs to
   `uv_mode` (stage 4a) and is not duplicated here. Whoever owns it must pass
   `0`, `1` or `2`; anything else silently takes the Z arm, exactly as the
   source does.
2. **The parity harness is local, and should not stay that way.**
   `surface_program::parity`'s `ParityGpu` is `pub(super)` to that module and
   the brief forbids touching it, so `frames.rs` carries its own ~200-line
   adapter/render/readback harness. Once the material-shader layers have landed,
   one shared `material_shader/parity_gpu.rs` is the right de-duplication — it
   will otherwise be copied twelve times.
3. **`FRAMES_WGSL` is self-contained.** It needs neither `SURFACE_PRELUDE_WGSL`
   nor any sibling layer to compile, which is why its parity test could run
   while other layers were mid-flight.
4. **The `--features offscreen` test build of the crate was red from siblings**
   (`weathering.rs` declaring a `mod parity;` with no such file;
   `macro_variation.rs` moving a `Vec<[f32; 4]>` into an `FnMut`) at the time
   this landed. To measure parity anyway, the real `frames.rs` was compiled
   verbatim by an isolated scratch crate via `#[path = ...] mod frames;` — no
   copy, no fork, the same bytes. Nothing in the repo was changed for it. Once
   the siblings compile, the in-repo invocation from the brief runs it directly.

## Reproducing

```sh
CARGO_TARGET_DIR=.../frames cargo test -p axiom-gpu-backend --lib \
  --features offscreen material_shader::frames -- --nocapture
```

Sixteen CPU tests run without the feature (they are what the coverage gate
measures); the four parity tests need it and a real adapter, and **assert** an
adapter was acquired rather than skipping.

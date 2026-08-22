# `material_shader::patches` — repair patches on vertical faces

Source: `C:/dev/Claude-of-Duty/src/materials/shader.js`

- the `OW_PATCH` block of `MAIN_FRAGMENT`, lines **449-492**;
- `owHash11`, lines **134-139** (the cell hash the block calls four times);
- the two shared wall axes it reads, lines **446-447**;
- `DEFAULT_PARAMS.patch`, line **742**: `[0, 2.6, 0.12, -0.08]` =
  `[ coverage 0..1, cell metres, albedo delta, roughness delta ]`;
- the compile-time gate, line **854**: `if ((p.patch?.[0] ?? 0) > 0) defines.OW_PATCH = ''`.

Target: `modules/axiom-gpu-backend/src/material_shader/patches.rs` — the WGSL, a
CPU reference, and CPU↔GPU parity on a real adapter, all in that one file.

## What the layer does

Somebody has replastered part of the wall. A repair is a rectangle in the plane
of the facade, a few percent off the surrounding mix in value, slightly smoother
because it is newer, hue-shifted (cool for a cement repair, warm for a patch of
the original mix that has weathered separately), and carrying a *trowel edge* —
a bright arris where the new render was feathered out. The rectangles come from
a cellular lattice over `(owSAxis, world Y)` with one candidate rectangle per
cell, the lattice itself wandered by `mac2.rg` so the cells are not a grid.

It writes three channels: `alb.rgb`, `orm.g` (roughness) and `owHeightS`.

## WGSL entry points

```wgsl
struct AxiomPatchChannels { albedo: vec3<f32>, roughness: f32, height: f32 };

fn axiom_patch_hash11(x: f32) -> f32
fn axiom_patch_smoothstep(e0: f32, e1: f32, x: f32) -> f32
fn axiom_patch_smoothstep2(e0: vec2<f32>, e1: vec2<f32>, x: vec2<f32>) -> vec2<f32>

fn axiom_patch_apply(
    world_pos: vec3<f32>,        // vOwWPos
    nw: vec3<f32>,               // owNw, face-corrected and normalized
    macro_second_rg: vec2<f32>,  // mac2.rg
    patch_p: vec4<f32>,          // owPatchP
    albedo_in: vec3<f32>,        // alb.rgb
    roughness_in: f32,           // orm.g
    height_in: f32,              // owHeightS
) -> AxiomPatchChannels
```

CPU mirror: `apply(world_pos, nw, macro_second_rg, patch_p, albedo, roughness,
height) -> PatchChannels`, plus `hash11`, `fract`, `step_ge`, `smoothstep`.

## What was pinned, and at what tolerance

**Bit-identity**, not a tolerance. On a real Vulkan adapter the worst absolute
lane delta over the 32-row sweep × 5 channels is **`0.0`** — the two sides agree
to the last bit — and `the_gpu_is_bit_identical_to_the_cpu_reference` asserts
exactly that while printing the measured delta on every run.

That is a derived choice, not a strict-for-its-own-sake one. The layer contains
no transcendental at all: `owHash11` is `fract`, two multiplies and two more
`fract`s; the rest is multiply/add/subtract, one divide, `floor`, `min`/`max`
and a hand-written smoothstep — every one correctly rounded in IEEE `f32`, with
the CPU reference at the same width. That leaves a device exactly two liberties,
and **neither has a middle ground**:

1. **Contracting `a*b + c` into an `fma`.** The exposed site is the hash
   argument, `cid.x * A + cid.y * B + C`. One ULP there is amplified by the two
   chaotic squarings into an `r0`/`r3` different in the *first decimal* — enough
   to flip `has` or `sgn` and paint a different wall.
2. **Evaluating `/` to 2.5 ULP** in `vec2(owSAxis, y) / cw`. On a facade 400 m
   out `pc` is of order 125, where 2.5 ULP is `2.4e-5`; the `1/fe ≈ 33` slope of
   the trowel feather turns that into `~1e-3` of patch mask, and it can move
   `pc` across an integer and change `cid` outright.

So a `1e-4`-shaped budget would catch neither case — it would only launder the
second into a pass. Either the device is exact or the building is different, and
the test says which.

Also pinned:

- **`coverage == 0` is a bit-identical no-op**, verified at the boundary on both
  sides rather than assumed: 2000 CPU rows across the lattice and 32 GPU rows,
  every channel compared by `to_bits()`. The mechanism is worth stating because
  it is not obvious from the algebra: the coverage draw becomes `step(1.0, r0)`,
  `r0` is a `fract` and therefore strictly below `1.0`, so `has` is `0.0`, `pm`
  is exactly `0.0`, `pm > 0.001` is false, and the inputs come back untouched.
  (In the *source* this is a compile-time fact — `OW_PATCH` is simply not
  defined — so making it a runtime fact is new work, not a transcription.)
- **`owHash11`'s output**, cross-checked against an independent `Math.fround`
  transcription of the same six GLSL lines written in Node from the source text
  (the `sky/` lesson: a second implementation in another language, not a reading
  of my own Rust). Exact `f32` agreement: `h(0) = 0`, `h(5.1) = 0.80285645`,
  `h(21.3) = 0.38511658`, `h(37.7) = 0.75`, `h(-53.9) = 0.50448608`.

## Traps, and what the source actually says

- **The cell hash is not a `sin` hash.** `owHash11` is the Dave-Hoskins scalar
  hash (`fract`, `p *= p + 33.33`, `p *= p + p`, `fract`). There is no
  `fract(sin(dot(…)) * K)` anywhere in this layer, so the libm-divergence budget
  the brief warned about does not apply — which is *why* bit-identity is
  reachable here. The four multiplier triples (`7.31/13.77/5.1`,
  `3.17/9.41/21.3`, `11.93/4.73/37.7`, `5.51/17.29/53.9`) are transcribed digit
  for digit and pinned by a test against the WGSL text.
- **The vertical-face test is not a comparison at all.** It is
  `smoothstep(0.72, 0.34, abs(owNw.y))` — a *reversed-edge* smoothstep. An
  exactly-vertical face (`nw.y == 0`), the common case in a building, evaluates
  `(0 - 0.72) / (0.34 - 0.72) = 1.894`, clamps to `1.0`, and is therefore
  **fully** patched; the ramp only bites past 20 degrees off vertical and is
  gone by 46. So the `>` vs `>=` question the brief flagged never arises on the
  facing test. The layer's one `>=`-flavoured test is the coverage draw,
  `step(1.0 - clamp(coverage, 0, 1), r0)`, where GLSL and WGSL `step` both mean
  `x >= edge` — inclusive, pinned by `step_is_inclusive_at_the_edge`.
- **`fract` is `x - floor(x)`.** `owSAxis` is a signed axis and `pc` is a world
  coordinate over a cell size, so negatives are the *normal* case here, not an
  edge case. Written as the subtraction; `fract_is_x_minus_floor_and_not_a_remainder`
  asserts it differs from `%` at `-2.25`.
- **`GLSL sign`** does not appear in this block. `sgn` is a ternary
  (`r3 > 0.48 ? 1.0 : -1.0`), not `sign()`, so the `signum` trap does not fire
  here — worth recording so nobody "fixes" it later.
- **Float grouping preserved.** `1.0 + sgn * owPatchP.z * pm` is
  `1.0 + ((sgn * z) * pm)`; `owHeightS + pm * 0.07 + lip * 0.05` is
  `(h + pm*0.07) + lip*0.05`; `vec2(owSAxis, y) / cw` stays a **division** per
  component and is never turned into a reciprocal multiply (a test asserts the
  WGSL text still contains the divide).
- **Dead source logic kept.** `pTint` is selected on `sgn > 0.0`, which cannot
  differ from the `r3 > 0.48` that produced `sgn`. Transcribed as written.

## Deliberate divergences from the source text (three, all narrow)

1. **`smoothstep` is written out by hand** on both sides instead of calling the
   builtin. WGSL leaves `smoothstep` **indeterminate** when `low >= high`, and
   the facing test relies on exactly that case; the hand expansion is the GLSL
   spec formula verbatim and removes the question. This also matches what
   `surface_program::parity` already does for `mix`, `dot` and `smoothstep`.
2. **`mix(vec3(1.0), pTint, pm)` is written out** as `x*(1-a) + y*a`, the spec
   formula for both languages, rather than left to a builtin a driver may
   implement as the fma-friendly `x + a*(y - x)`.
3. **The CPU `clamp01` uses `f32::clamp`, not `.max(0.0).min(1.0)`.** For every
   finite input these agree; on `-0.0` `f32::clamp` agrees with WGSL *better*
   (WGSL's `max(-0.0, 0.0)` returns `-0.0`, Rust's `f32::max` returns `0.0`).
   `NaN`, the one input where they genuinely differ, is unreachable: every
   argument is finite and the only division is by `cw`, already floored at `0.4`.

The `if (pm > 0.001)` guard is **kept** and is not an optimisation — it leaves a
real `~1e-4` discontinuity at the threshold. In WGSL it stays an `if` (shader
text is data, and the brief says write it as the source writes it); in the
branchless Rust it is a table selection between the untouched and the written
channels, with both always evaluated and neither able to trap.

## What the layer needs from siblings (all taken as explicit arguments)

- **`mac2.rg`** — the second macro-texture sample, which wanders the lattice.
  Owned by `macro_variation.rs`. Assumed name at the call site:
  `macro_second_rg`.
- **`owVert` / `owSAxis`** (source lines 446-447) are computed *outside* the
  `#ifdef OW_PATCH` block and are shared with the runoff layer in `weathering.rs`.
  They are derived inside `axiom_patch_apply` from `world_pos` and `nw` so this
  layer is complete and testable alone. The expressions are identical, so
  hoisting them into a shared prologue later is exact — worth doing, since
  weathering needs the same two values.
- **`owHash11`** is likewise shared with `owRunoff` in `weathering.rs`. It is
  emitted here as `axiom_patch_hash11` so two layers can be composed into one
  `axiom_surface` without a duplicate WGSL definition. If the orchestrator
  hoists it, the name should become neutral and both call sites updated.

Nothing else is consumed and nothing is reached for: no globals, no
`params.slots`, no assumed binding index.

## Verification actually run

| gate | result |
|---|---|
| `cargo test -p axiom-gpu-backend --lib material_shader::patches` (default features) | 13/13 green |
| same, `--features offscreen` | 16/16 green, incl. 3 GPU tests on a real Vulkan adapter |
| `cargo llvm-cov --branch -p axiom-gpu-backend --lib` | `patches.rs` **100.00%** regions / lines / functions, **zero** branch regions |
| `cargo clippy -p axiom-gpu-backend --all-targets` (CI's config) | zero findings in `patches.rs` |
| `rustfmt --check` | clean |

Two notes on how those were run.

**The `--features offscreen` build of the crate was broken by three siblings
mid-flight** (`weathering.rs` declares a `mod parity;` whose file does not exist,
`frames.rs` has a missing `AxisProbe` field, `macro_variation.rs` moves a
captured `Vec`). Those are other agents' work in progress, so rather than touch
them the parity run was done in a throwaway `git worktree` of `HEAD` — where the
whole `material_shader/` tree is still unwritten — with only `patches.rs` and a
one-line `mod.rs` copied in. Same file, same toolchain, real adapter. The tests
live in the repo file and will run there the moment the siblings compile.

**The layer is gated `#![cfg(any(test, target_arch = "wasm32", feature =
"offscreen"))]`**, the shape `mip_chain`/`texture_sampling` already use in this
crate. That keeps it present and measured under the coverage gate (which runs
tests) and under the arms that will compose it, while keeping a plain
`cargo build` free of dead-code warnings for a layer nothing consumes *yet*.
Under `--features offscreen` without `cfg(test)` there is still a dead-code
warning per item — the honest signal that `mod.rs` has not wired the layer in.
It disappears on wiring. CI's clippy runs default features, where the file is
clean.

## Orchestrator wiring

Nothing to add to `mod.rs` — `pub(crate) mod patches;` is already there. When the
layers are composed into `axiom_surface`, splice `PATCHES_WGSL` and call
`axiom_patch_apply` after the macro-variation layer has produced `mac2` and
before weathering (the source's order). Consider hoisting `owHash11`, `owVert`
and `owSAxis` out of `patches.rs` and `weathering.rs` into a shared prologue at
that point; the expressions are byte-identical, so it is a rename, not a
re-derivation.

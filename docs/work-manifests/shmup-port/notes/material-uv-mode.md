# `material_shader/uv_mode` — the uv construction

The `uvMode` layer of Claude-of-Duty's `src/materials/shader.js`, ported into
`modules/axiom-gpu-backend/src/material_shader/uv_mode.rs`: the WGSL, a CPU
reference, and CPU↔GPU parity on a real adapter.

Source sites transcribed:

| source | lines | what |
|---|---|---|
| `owAxisFrame` | ~185-199 | the `s` handedness vector, the three per-axis uvs, the tile transform |
| `MAIN_FRAGMENT` head | ~253-267 | `owFaceDir`, `owNw`/`owNp`, the `OW_OBJECT_SPACE` space selection |
| `MAIN_FRAGMENT` `OW_TRIPLANAR` | ~276-279, ~305-307 | the blend weights, the detail-plane choice |
| `MAIN_FRAGMENT` `OW_MESH_UV` / planar | ~325-334 | mesh uv, the dominant-axis chain |
| `extendMaterial` | ~794 | `tileScale = uvMode === 'mesh' ? scale : 1 / scale` |
| `DEFAULT_PARAMS` | ~697-705 | `uvMode`, `localSpace`, `scale`, `offset` |

## WGSL entry points

All free functions, explicit arguments, no globals and no assumed binding index.
`tile` is the source's `owTile`: `.xy` = tiles per metre (already divided,
CPU-side), `.zw` = offset. `local_space` is `OW_OBJECT_SPACE` as a runtime flag
(`> 0.5`). `face_dir` is `owFaceDir` (+1 front, -1 back).

```wgsl
fn axiom_uv_projection_pos(object_pos: vec3<f32>, world_pos: vec3<f32>, local_space: f32) -> vec3<f32>
fn axiom_uv_projection_normal(object_normal: vec3<f32>, world_normal: vec3<f32>, face_dir: f32, local_space: f32) -> vec3<f32>
fn axiom_uv_tile(uv: vec2<f32>, tile: vec4<f32>) -> vec2<f32>
fn axiom_uv_axis_sign(n: vec3<f32>) -> vec3<f32>
fn axiom_uv_axis_project(p: vec3<f32>, n: vec3<f32>, axis: i32) -> vec2<f32>
fn axiom_uv_axis(p: vec3<f32>, n: vec3<f32>, axis: i32, tile: vec4<f32>) -> vec2<f32>
fn axiom_uv_dominant_axis(n: vec3<f32>) -> i32
fn axiom_uv_planar(p: vec3<f32>, n: vec3<f32>, tile: vec4<f32>) -> vec2<f32>
fn axiom_uv_triplanar_weights(n: vec3<f32>) -> vec3<f32>
fn axiom_uv_triplanar_detail_axis(n: vec3<f32>) -> i32
```

`mesh` mode has **no entry point of its own**: it is exactly
`axiom_uv_tile(in.uv, tile)` — the source's `vMapUv * owTile.xy + owTile.zw`
verbatim. A second name for the same two operations would be a shim.

The Rust mirrors are one-for-one (`projection_pos`, `projection_normal`,
`tile_uv`, `axis_sign`, `axis_project`, `axis_uv`, `dominant_axis`, `planar_uv`,
`triplanar_weights`, `triplanar_detail_axis`), plus `UvMode`, `tile_scale` and
`tile`, all `pub(crate)` and all on `[f32; N]` arrays rather than
`axiom_math` vectors so the grouping of every expression is under this file's
control.

## What the traps actually turned out to be

**`scale` divides — and the source divides it on the CPU.** `extendMaterial`
computes `1 / p.scale` in JavaScript and uploads it as `owTile.xy`; the shader
only ever multiplies. So the division lives in `tile_scale`, in Rust, and it is a
division. It is evaluated in `f64` and rounded to `f32` because that is what
JavaScript plus `three`'s upload does — storage width is part of the algorithm.
A 400k-sample search over `scale ∈ 0.01..50` found **no** `f32` value where that
differs from an `f32` division, so the choice is not observable; it is made
because it is what the source does, not because it changes a bit.

In `mesh` mode `scale` is a repeat count and is **not** divided. That is the
source's own ternary, and getting it wrong would invert the tiling of every
mesh-uv material.

**The handedness is `step`, not `sign`.** `owAxisFrame` builds `s` from
`mix( vec3( -1.0 ), vec3( 1.0 ), step( 0.0, n ) )`. GLSL `step(edge, x)` is
`x < edge ? 0 : 1`, so **both** signed zeros select `+1`. `f32::signum` gives
`-1.0` for `-0.0` and GLSL `sign` gives `0.0` for either — both wrong, and this
is not an exotic input: `projection_normal` of an axis-aligned normal on a
**back** face produces exactly `[-0.0, -0.0, -1.0]`, so the wrong function would
have mirrored the tangent basis on every back-facing axis-aligned fragment. The
first draft of the *test* asserted `signum`'s answer and the reference caught it.

**Per-axis flips.** Axis 0 flips `-p.z * s.x`, axis 1 flips `-p.z * s.y`, axis 2
flips `p.x * s.z`; the other lane is `p.y` (axes 0 and 2) or `p.x` (axis 1),
unflipped. Pinned by `each_axis_projection_flips_the_component_the_source_flips`,
which checks both faces of all three axes: the flipped lane changes sign with
the face and the other does not, which is what stops the `+X` and `-X` faces of
a box being mirror images.

**Two tie rules, and they disagree.** The planar chain is a nested ternary on
strict `>`; the triplanar *detail*-plane choice is `y > max(x, z)` then
`x > z`. At `|n| = (0.5, 0.5, 0.1)` the planar chain picks **Y** and the detail
chain picks **X**. Unifying them would be a quiet retexturing of every triplanar
surface, so both are transcribed as written and the divergence is asserted in
`the_triplanar_detail_axis_is_not_the_planar_dominant_axis`.

**The blend exponent is the look.** `pow( an, vec3( 5.0 ) )` normalised by
`max( w.x + w.y + w.z, 1e-4 )`. Kept as `pow`, not expanded into a multiply
chain — that would be a re-association, and it is also the one place where a
"tidier" transcription would silently change the sharpening. The sum is
`( x + y ) + z` and the normalisation is a division, both literal. The `1e-4`
floor is a floor on the *divisor*, not a clamp on the result: at `n = (0.1, 0, 0)`
the weights do **not** renormalise to unity, and the test pins that too.

## Parity

`uv_mode::parity` (compiled only under `--features offscreen`) acquires a real
adapter and asserts one was acquired — never skips. Six fragment entry points,
each returning four lanes of the **composed** chain (space selection → normal →
axis choice → uv), over 24 contexts chosen for: every axis dominant, every sign,
exact ties on each pair and on all three, zero components, non-unit normals, both
face directions, both spaces, and tiles built from all three `UvMode`s with a
non-power-of-two scale and a non-zero offset.

Measured worst absolute lane delta, on Vulkan (discrete), committed as
`MEASURED_WORST_DELTA` and re-taken every run:

| entry point | measured | declared | why it is not zero |
|---|---|---|---|
| `uv_pos_fs` | `0` | `0` | bit-for-bit; a `select` and a comparison chain |
| `uv_normal_fs` | `5.96e-8` | `3e-7` | `normalize`: reference divides by `length`, GPU may use `rsqrt` |
| `uv_weights_fs` | `1.19e-7` | `6e-7` | `pow`: both sides approximate, different polynomials |
| `uv_planar_fs` | `7.63e-6` | `3e-5` | `fma` contraction of `uv * tile.xy + tile.zw` |
| `uv_axes01_fs` | `7.63e-6` | `3e-5` | same |
| `uv_axis2_fs` | `7.63e-6` | `3e-5` | same |

The uv figure looks large and is not: `7.629e-6` is exactly `2^-17`, **one ulp**
of a uv near 85 — the largest this sweep produces, from a `mesh`-mode repeat
count of 12 over a position four metres out. Relative, it is `9e-8`. It is the
adapter contracting the multiply-add into a single-rounding `fma`, which WGSL
permits and the reference does not do; the alternative — writing the reference as
`mul_add` — would be *guessing* the hardware rather than measuring it, so the
budget carries it instead.

Notably, `pow` needed only one ulp here, not the transcendental-sized budget
`surface_program::parity_transcendental` gives `FieldOp::Pow` (`3e-5`). That
budget is a magnitude artefact of that case's `~104`-sized output, not an
accuracy claim, and this layer's weights live in `0..=1`.

Each entry point carries its **own** tolerance rather than sharing one, because
a shared number would be the loosest of the six.
`the_tolerances_are_not_looser_than_the_hardware_needs` holds the live delta
against the committed record (`DRIFT_LIMIT` 2×), against the declared tolerance,
and against a `SLACK_LIMIT` of 10× — a budget more than an order of magnitude
above what the hardware needs **fails**, exactly as in
`surface_program::parity_transcendental`. Every failure quotes the whole
six-entry run, so one red test hands over the complete re-measurement.

## Not ported here, and why

- `owAxisFrame`'s `T`/`B`/`N`, `owOrthonormalise`, `owTangentFrame` — the
  `material_shader::frames` layer. `axiom_uv_axis_sign` is exported so the frame
  basis and the uv are built from the same `s` by construction rather than by
  coincidence; `frames` should call it rather than restate the `mix`/`step`.
- `owFaceDir` itself (`gl_FrontFacing`). `SurfaceIn` carries no front-facing lane,
  so `face_dir` is a parameter of `axiom_uv_projection_normal`. Whoever composes
  `axiom_surface` has to supply it — a `@builtin(front_facing)` input on the
  fragment stage, or `1.0` if the pass never draws back faces. **This is the one
  input this layer needs that the seam does not yet carry.**
- `vOwViewDirP` (the object-space parallax view vector) — POM's, not this
  layer's. `SurfaceIn.view_dir` is world-space; under `localSpace` the POM layer
  will need the object-space one, which is a separate question for `pom`.
- `owDist` and the `detFade`/`pFade` distance ramps — `detail` and `pom`.

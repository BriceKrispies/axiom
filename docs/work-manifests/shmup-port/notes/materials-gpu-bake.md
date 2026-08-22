# materials/gpu-bake — the nineteen generators on the GPU

Slice: the GPU half of `generator.js`, spanning host layer → gpu-backend module
→ `apps/shmup`. Written under `12-final-wave-brief.md`: **nothing here has been
compiled or run.**

Files written:

| file | tier | what |
|---|---|---|
| `crates/axiom-host/src/procedural_bake.rs` | layer | `ProceduralBakeRequest`, `ProceduralBakeMaps`, `BakeOutput` — the backend-neutral bake contract |
| `modules/axiom-gpu-backend/src/texture_bake.rs` | module | the forge: WGSL header/footer/Sobel, formats, the four passes, the read-back |
| `apps/shmup/src/materials/wgsl/{mod,noise,arch,ground,metal,organic}.rs` | app | 1,885 lines of GLSL transcribed to WGSL |
| `apps/shmup/src/materials/gpu_bake.rs` | app | the bake list (`index.js`) and the channel repack |
| `apps/shmup/tests/materials_gpu_bake_port.rs` | app | GPU vs the CPU goldens |

## The verdict: hand-written WGSL, not the field algebra

`01-engine-gaps.md` records the load-bearing decision as *"bake-time texture
generation belongs in the field/proc-texture path … the 19 procedural surface
generators are straight-line noise math with no sampling and no derivatives …
the node budgets need raising to hold them."*

**Having read all 1,885 lines of the GLSL, the premise is wrong and the decision
has to be revised, not executed.** The generators are not straight-line:

| what the algebra lacks | what the generators do with it |
|---|---|
| loops | `owWorley` 3×3; `owVoronoiEdge` **two-pass** 3×3 then 5×5 centred on a cell the first pass found at runtime; `owFbm`/`owRidged`/`owBillow` 4 octaves; `FOLIAGE` 3×3 over leaf cells |
| comparison / selection | `if (d < f1) … else if (d < f2)` carrying a `vec2` id payload; `if (dot(diff,diff) > 1e-5)`; `FOLIAGE`'s nested `if (cover > 0.01) { if (depth > bestDepth) …}` over five accumulators |
| division | `s / max(n, 1e-4)` in all three fbms; `owRemap`; `owSRGB`'s `/1.055` and `/12.92`; `d / max(pinch, 0.3)` |
| `floor` / `fract` / `mod` | every lattice access in every function — this is what makes the library *periodic*, which is the one property it exists for |

`FieldOp`'s catalog is 27 operators and its excluded-operator table names
`Div`, `Step`, `If`/`Select`/`Compare` as deliberate permanent exclusions
("a comparison operator is the seed of control flow in a language that must stay
branchless end to end"). `Floor`/`Fract`/`Mod` are not excluded — they were
never contemplated. And `FieldOp::Fbm` is 3D **non-periodic** FNV-1a gradient
noise with no `per` parameter, so it cannot stand in for `owFbm` even where the
shape would fit.

### The measured node counts

A static count of the fully inlined, fully unrolled **scalar** expression graph
each `owSurface` becomes — every binary operator and builtin counted once per
lane, shared subexpressions counted once (so this is a *lower* bound), loops
expanded at their real trip counts (fbm 4, Worley 9, Voronoi-edge 9+25):

```
owHash12        19     owNoise        200     owWorley        747
owHash22        19     owFbm          840     owVoronoiEdge  4352
owGrad2         33     owWarp       1,688     owCracks       6,930
```

```
PLASTER       43,403     METAL_PAINTED  14,351     WOOD           9,398
CONCRETE      35,555     RUBBER         12,999     CORRUGATED     8,252
ASPHALT       28,558     METAL_RUST     12,316     GRAVEL         7,761
BRICK         25,468     FABRIC         10,591     BURLAP         7,225
DIRT          15,710     TILE            9,859     SAND           7,064
                         METAL_BRUSHED   9,604     GLASS          4,337
                                                   FOLIAGE        2,113*
```

`* FOLIAGE`'s own 3×3 leaf loop is not expanded by the counter, so 2,113 is a
floor; the real figure is ~9× the loop body.

Against `MAX_NODES = 256` per graph and `MAX_SURFACE_NODES = 256` across a
**whole surface, every channel and every layer**. The median generator is 170×
over. **Raising the budget (G15) would not help**: the missing operators are
semantics, not size, and adding `floor`/`fract`/`mod`/compare/divide to the
algebra is a change to the language, not to a constant. `SurfaceKind`'s own
module doc already makes exactly this argument for the runtime shader — "the
algebra has no loops, no derivatives and no sampling, and its branchlessness is
the Branchless Law itself, so those absences are immovable" — and it applies
verbatim to the bake.

So: hand-written WGSL, in the same shape and the same tier as
`material_shader/`. The one thing carried over from the field decision is the
*precedent*: `wgsl_template.rs`'s `SURFACE_PRELUDE_WGSL` already ships a
hand-written WGSL noise library pinned by a parity test, and this is the second.

## The three tiers, and why the seam is where it is

`generator.js` already draws the line this slice follows: the `TextureForge`
(targets, formats, four draws, Sobel, read-back) knows no surface, and the
`glsl/` directory knows no renderer.

* **`crates/axiom-host`** owns the *contract*, and it has to. A bake request
  travels app → engine facade → whichever backend holds a device, and Module Law
  #8 lets `axiom-gpu-backend` publish exactly one facade — so a request type
  declared there would be unnameable by everyone who has to send one. This is
  the same argument `MaterialTexture`'s module doc makes, in the same file
  neighbourhood.
* **`modules/axiom-gpu-backend`** owns the *execution*. Its pure decisions
  (the WGSL splice, the Sobel strength, the formats, the uniform byte layout,
  the row un-padding) are in un-`cfg`'d functions so the coverage gate sees them
  on a build with no `wgpu`; only the device work is behind
  `#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]`, exactly as
  `offscreen.rs` and `hdr_target.rs` are shaped.
* **`apps/shmup`** owns the *content*: the nineteen `owSurface` bodies, the
  noise library, the bake list, and the translation into `upload::BakedLibrary`.
  Game content never moves into the engine.

## Transcription: what was actually done to the GLSL

Four independent transcriptions (one per `surfaces-*.js`) plus the noise library,
each written **from the GLSL text**, none allowed to read the existing Rust CPU
port. That is deliberate: `11-render-fanout-brief.md` measured ten defects in
`sky/` where the Rust and the "independent" check shared one misreading because
one hand wrote both. Here the CPU port *is* the independent reading, and the
parity test is where they meet.

Structural changes, exhaustively:

1. **GLSL `out` parameters → `ptr<function, T>`**, with each body opening
   `var alb = *albOut; …` and closing `*albOut = alb; …`. That pair *is* GLSL
   out-parameter semantics and it lets every body line stay verbatim.
2. **`uSeed`/`uTintA`/`uTintB`/`uParam` → `U.seed`/`U.tint_a`/`U.tint_b`/
   `U.param`.** WGSL has no loose uniforms.
3. **`macro` is a WGSL reserved word** and is a hard parse error. Six locals are
   renamed: `macro_` (CONCRETE, PLASTER, ASPHALT, DIRT), `macroNoise`
   (METAL_BRUSHED), `macroF` (FABRIC, BURLAP, RUBBER). Nothing else is renamed.
4. **Scalar broadcast on `+`/`-` made explicit.** WGSL has none: GLSL `v + 1.0`
   is `v + vec3<f32>(1.0)`. ~60 sites. `*` and `/` by a scalar are left as
   written.
5. **`mix`/`clamp`/`step`/`smoothstep`/`mod`/`sign` written out** as `owMix`,
   `owClamp`, `owStep`, `owSmoothstep`, `owMod`, `owSign` — the precedent
   `surface_program::emit` sets, and the shape the CPU twin already has
   (`gl_mix`, `gl_mod`, …). Two of them genuinely differ from their WGSL
   namesakes: GLSL `mod` is floored (`x - y*floor(x/y)`) and lattice
   coordinates go negative, and GLSL `sign` returns `0.0` for zero, which
   `CORRUGATED`'s ridge crossings depend on.
6. `atan(y, x)` → `atan2(y, x)`; `float(x)` → `f32(x)`; explicit `vec2<f32>` etc.
   constructors; multi-declarator lines split in source order.

No division was rewritten as a reciprocal multiply, no multiply chain was
re-associated, no grouping was tidied. **One place the existing CPU port did do
this and the WGSL does not**: `bake.rs::sobel` writes the source's
`dx / uTexel.x` as `dx * size`. It is exactly equal for a power-of-two `size`
(`1/size` is then exact), so it is not a defect — but the WGSL writes the
division, because the source's grouping is the specification.

### Source defects found, ported faithfully, named

| where | what |
|---|---|
| `CORRUGATED:306` | `washer * 0.0 +` — a disabled term contributing exactly zero |
| `CORRUGATED:304` | `ao -= (washer - screw) * 0.35` can *raise* `ao`; the smoothstep radii keep the difference ≥ 0 in practice, but it is unguarded |
| `METAL_PAINTED:146` | `mtl = mix(0.0, 1.0, …)` **overwrites** the accumulator instead of blending; harmless only because `mtl` is still 0 there |
| `WOOD:95` | `nd = length(nf * vec2(3,1) / vec2(3,1) * vec2(1,1))` is self-cancelling **and** immediately overwritten; `nf` exists only to feed it |
| `FOLIAGE:290` | the first `cover` is overwritten by the serrated one on the next line |
| `FOLIAGE:313` | `bestH` is accumulated and **never read** — the epilogue writes `h = bestCover` (the cutout mask), so the leaf height has nowhere to go. Probably a real bug in the original |
| `BRICK:317`, `PLASTER:478`, `ASPHALT:46`, `ASPHALT:75` | period/domain mismatches (`p.y * 2.3` against `P.y * 2.0`) — these noises do not tile. `CONCRETE:162` does the same thing correctly, which is what makes the others look like typos |
| `ASPHALT:73` | the tyre-polish band uses raw `uv`, not the seeded `p`, so it lands in the same two places for every seed. Also `uv.x * 1.0` |
| `BRICK:196-198` | `rnd2.w` computed, never read |

## Correcting a measurement's attribution

`upload::RUNTIME_BAKE_SIZE`'s doc attributes the CPU bake's cost to `ow_hash22`
being "the classic `fract(sin(dot(…)))` GLSL hash". It is not: `noise.js:11`
says *"hashes are sin-free (Dave Hoskins style) — sin() based hashes band badly
on Apple GPUs"*, and `ow_hash22` is pure `fract`/multiply churn. The
transcendentals are in `owGrad2` — one `cos` and one `sin` per lattice corner,
four corners per `owNoise`, four octaves per fbm, so a four-octave fbm is 32 of
them and a Worley is 18 hashes with none. The measurements (16.6 s / 232 s /
~930 s) and the conclusion stand; only the attribution is wrong, and it is worth
correcting because it is the sentence a future agent will read when deciding
whether a CPU rewrite could close the gap. It cannot: the gap is 1024²-way
parallelism.

## Storage width, colour space, and the v axis

Three format decisions are part of the algorithm, not preferences:

1. **The height scratch is half-float** (`generator.js:180-186`) because an
   8-bit height field stair-steps the Sobel. `height_format(half_float_targets)`
   mirrors the source's own `canHalf ? HalfFloatType : UnsignedByteType` probe,
   and the 8-bit fallback is `Rgba8Unorm` (linear), never sRGB — the source's
   fallback target is `RGBAFormat`/`NoColorSpace`, and an sRGB encode of a
   height field would silently corrupt the Sobel's input.
   `a_half_float_scratch_is_finer_than_an_eight_bit_one` pins the difference.
2. **The albedo target is sRGB unless `linear_albedo`** (`generator.js:276`),
   and the hardware encodes on write. Because the encode and the binding are
   chosen by the same flag, **a bake through this path cannot land in gap
   G16** ("baked field textures are written linear and bound as
   `Rgba8UnormSrgb`, so a baked tile reads darker"). The two shared maps set
   `linear_albedo` for the source's own stated reason: "the detail map is DATA,
   not colour".
3. **ORM and normal are linear 8-bit** (`NoColorSpace`).

**The v axis** is the one place the port cannot be literal. A WebGL render
target's row 0 is its *bottom* row, so the source's `vUv` varying over a
`PlaneGeometry(2,2)` makes row 0 the `v ≈ 0` row. A WebGPU target's row 0 is its
*top* row. Deriving the UV from `@builtin(position) * inv_size` —
`((x+0.5)/size, (y+0.5)/size)` — reproduces the source's mapping **and** matches
`bake.rs::texel_uv`, whose row 0 is likewise `v ≈ 0`. A `-y` clip-space varying
instead would flip every normal's green channel and mirror every anisotropic
surface. Pinned by `the_uv_of_row_zero_is_the_low_v_row`.

## The tolerance, and why it is a fraction rather than a maximum

**Unverified — nothing in this slice has run.** The budget is derived from the
two sides' arithmetic:

| term | size |
|---|---|
| `f64` CPU vs `f32` GPU through the hash → `owGrad2` → fbm chain, sRGB-encoded | ≈ 0.07 LSB |
| the `f16` height scratch vs the CPU's `f32`, through the Sobel and `size·relief/worldSize` | ≈ 0.07 LSB |
| 8-bit quantisation when the two sides straddle a rounding boundary | ± 0.5 LSB, on a small fraction of texels |
| **a hard `step()` edge or a Worley `d < f1` tie flipping** | **unbounded** — `gritA * 0.26` on a height is 66 LSB |

The first three are why the per-channel allowance is 2 LSB (4 for normals, which
are a *derivative*). The fourth is why the test caps an **outlier fraction**
rather than a maximum: a `step()` is discontinuous, ~1e-5 of texels straddle
each edge, and the library has tens of such sites. Bounding the magnitude there
would mean fitting a tolerance to whichever texel happened to flip — the thing
`11-render-fanout-brief.md` explicitly forbids.

Predicted, to be replaced by measurement on the first green run:

```
albedo / ORM   mean ≤ 0.75 LSB, ≤ 0.2 % of channels past 2 LSB
normal         mean ≤ 1.5  LSB, ≤ 2   % of channels past 4 LSB
```

Every assertion prints the measured mean, max and outlier fraction, so the first
run hands back the real numbers. An anti-vacuity guard rejects a comparison
where the GPU map spans ≤ 8 LSB — two flat maps agree perfectly and prove
nothing.

`the_noise_library_agrees_with_its_cpu_twin` bakes `owHash12`/`owFbm01`/
`owWorley.x`/`owVoronoiEdge` into four linear channels and compares them
separately, so a hash typo reads as a hash typo rather than as eighteen
simultaneous generator failures.

## An expired deferral, found

`notes/materials-upload.md` records that four of the five maps are "produced but
unbindable" and specifies the `MaterialTexture` extension they need. **That
extension has landed.** `axiom_host::MaterialTexture` now carries `with_normal`,
`with_orm_height`, `with_detail` and `with_macro_field`, and `axiom::Material`
carries the four ids (`with_normal_texture`, `with_orm_texture`,
`with_detail_texture`, `with_macro_texture`) that resolve them through the
existing `add_texture_data` store. Every map this slice produces is bindable
today with **no further engine change**.

What `scene::app` still does is call `upload::bake_albedo_maps` — albedo only,
64², CPU — which is now leaving four maps and 99.6 % of the resolution on the
floor. That is the visible defect in
`reference/axiom-street-agx.png`: a 64 px tile stretched over a building reads
as horizontal streaking.

## Not done, with its expiry check

The **execution lane** is offscreen-only. `bake_on_device` compiles for
`wasm32`, but nothing carries a request to it in the browser. To close it:

1. `modules/axiom/src/app/` — an install-time bake request beside
   `add_texture_data`, carrying `axiom_host::ProceduralBakeRequest`.
2. `modules/axiom-windowing` + `modules/axiom-gpu-backend/src/live_gpu_binding.rs`
   — run it on the live device, which is the whole point of baking on the device
   the textures are sampled on.
3. `apps/shmup/src/scene/app.rs` — switch from `upload::bake_albedo_maps` to
   `gpu_bake::plan` + `assemble`, and set the four extra ids per `Material`.

**Expiry check**: if (1) and (2) land and `scene/app.rs` still calls
`bake_albedo_maps`, this slice is a deferral that quietly stopped being one —
the failure mode this port has already hit four times, most expensively as a
765-line `interiors.rs` that was never compiled. The check is one grep:
`bake_albedo_maps` in `apps/shmup/src/scene/`.

Also still open, unchanged from `materials-upload.md`: binding 5's packing
drops the source's `detailAlbedo.r` (the micro albedo the shader's
`(dTex.r - 0.5) * 1.25` term reads), so half the micro layer stays dead until
`material_shader/compose.rs` either reconstructs the detail normal's `z` and
frees a channel, or a binding 7 is added.

## Wiring the orchestrator must add

```
crates/axiom-host/src/lib.rs:           mod procedural_bake;
crates/axiom-host/src/lib.rs:           pub use procedural_bake::{BakeOutput, ProceduralBakeMaps, ProceduralBakeRequest};
crates/axiom-host/layer.toml:           introduced_capabilities += "ProceduralBakeRequest"
modules/axiom-gpu-backend/src/lib.rs:   mod texture_bake;   // unconditional: the decisions must be covered
apps/shmup/src/materials/mod.rs:        pub mod wgsl;
apps/shmup/src/materials/mod.rs:        pub mod gpu_bake;
apps/shmup/Cargo.toml [dependencies]:   axiom-host = { path = "../../crates/axiom-host" }
apps/shmup/Cargo.toml [dev-dependencies]: axiom-gpu-backend = { path = "../../modules/axiom-gpu-backend", features = ["offscreen"] }
apps/shmup/app.toml:                    allowed_layers += "host";  allowed_modules += "gpu-backend"
```

plus, in `modules/axiom-gpu-backend/src/gpu_backend_api/mod.rs`, beside
`render_offscreen_rgba`:

```rust
/// Bake one procedural texture set on the process-wide device — the port of
/// `TextureForge.build` (`generator.js:260-321`). `library_wgsl` is the shared
/// noise library the surface program is compiled against. `None` when the
/// machine has no adapter, the same contract `render_offscreen_rgba` has.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
pub fn bake_procedural_texture(
    library_wgsl: &str,
    request: &axiom_host::ProceduralBakeRequest,
) -> Option<axiom_host::ProceduralBakeMaps> {
    crate::texture_bake::bake_offscreen(library_wgsl, request)
}
```

# `material_shader/cloth` — fabric transmission, and where it is allowed to live

The cloth layer of the runtime material shader: the `OW_CLOTH` block of
`MAIN_FRAGMENT` (`shader.js:598-618`), the `OW_CLOTH_LIGHT` macro
(`shader.js:120-126`), the `CLOTH_LIGHT` chunk that `OVERRIDES` splices after
`<lights_fragment_end>` (`shader.js:650-663`), the `OVERRIDES` table itself
(`shader.js:668-697`), and `DEFAULT_PARAMS.cloth` (`shader.js:747`).

File: `modules/axiom-gpu-backend/src/material_shader/cloth.rs`.

---

## 1. Placement conclusion, first

**Transmission becomes a seventh `SurfaceOut` channel — a scalar — consumed by
one gated term inside `scene_wgsl`'s existing light loop.** Not a fourth
`LightingModel` variant, and not the `emission` channel.

The full argument is in the module header of `cloth.rs`, at the site, where a
future reader will actually be. The short form:

### 1.1 The term separates into three parts, and they have three homes

```glsl
owTrans += owCl.color * ( owBackLit * ( 0.30 + 0.90 * owFwd * owFwd ) );  // per light
reflectedLight.directDiffuse += owTrans * diffuseColor.rgb
  * ( owClothP.x * clamp( owORM.r, 0.0, 1.0 ) );                          // once
```

| part | what it is | where it belongs |
|---|---|---|
| `0.30 + 0.90 f²` | a fixed wrap + forward-scatter lobe, no authored numbers | a constant in `scene_wgsl`, beside `SPECULAR_POWER` |
| the sum over directional lights | needs `L` and the light colour | the light loop in `fs` |
| `clothP.x * clamp(ao, 0, 1)` | a per-surface amount | a `SurfaceOut` channel |

Written that way it stops being an exception to "a surface program supplies
channel values, never a way of being lit". The surface still supplies only a
value; the lighting stage still owns all the lighting.

### 1.2 Why not a fourth `LightingModel`

- `emit_lighting::lighting_model_function` emits a **nullary constant**,
  `fn axiom_lighting_model() -> u32`. A variant carries no data, and the amount
  *is* data — so a variant would need a value carrier anyway. Two mechanisms
  where one suffices, plus a new variant in the `axiom-surface` layer and a new
  `RenderPipelineKind` mirror in `axiom-render`.
- `Unlit`/`Lambert`/`LambertSpecular` is not a set of BSDFs. It is a **monotone
  ladder** of how much of the standard maths a surface takes, lowered to
  `diffuse_gate` and `specular_gate`. Transmission is **orthogonal** to that
  ladder — a cloth awning still wants Lambert *and* its specular sheen. A fourth
  rung forces an author to pick one, and the moment a second orthogonal term
  appears the closed set is 2×3 = 6. That is exactly the variant multiplication
  `emit_lighting`'s own header refuses.

### 1.3 Why not `emission` — the near miss, and what it costs

The **slot** is right, and this is worth saying plainly rather than dismissing.
`emission` is added after every light term, unattenuated by N·L, ambient or
shadow, and before fog — which is precisely where three's
`reflectedLight.directDiffuse +=` at `<lights_fragment_end>` sits relative to
`<fog_fragment>`. The accumulation semantics match to the letter, and the
source's term is deliberately unshadowed too (§4.2).

What emission cannot supply is the **light rig**. `axiom_surface` runs before
`fs` has looked at a single light. So an emission-encoded transmission must
either bake a sun direction into the parameter block — a second, staleable copy
of the frame's own light, which the engine has no mechanism to keep in sync —
or drop `owBackLit`/`owFwd` entirely. Dropping them costs:

- **The whole effect.** The term exists so the awning is dark when the sun is in
  front of it and blazes when the sun is behind it. A constant emission glows
  identically at every sun angle: the "painted card with a knife edge" the
  source's own comment says this term was written to avoid.
- **The fold read.** `0.30 + 0.90 f²` is `1.20` looking along the beam against
  `0.30` across it — 4×. That gradient across one awning *is* the drape.
- **Night.** The sum is over every directional light, so the moon lights fabric
  from its own direction. One baked vector cannot.
- **Silhouettes.** A constant cannot go dark as the surface turns away, so a
  back-lit awning and a front-lit one read identically at grazing angles.

### 1.4 The price of the channel, stated

Unlike a pipeline variant, the new term's ALU runs for **every fragment of every
draw in the engine**, gated to zero: two dot products, a square, two
multiply-adds per light, plus one vec3 madd after the loop. That is the same
bargain the three lighting models and the twelve capability bits already strike
and it is what the no-variant doctrine is made of — but it is not free, and the
orchestrator should take it knowing the number.

### 1.5 The exact change, for the orchestrator to make

I did not touch `scene_wgsl.rs` or `surface_program/`. Four edits:

**(a) `surface_program/wgsl_template.rs`, `SURFACE_PRELUDE_WGSL` — `SurfaceOut`
gains one scalar:**

```wgsl
struct SurfaceOut {
    base_color: vec4<f32>,
    roughness: f32,
    metallic: f32,
    normal: vec3<f32>,
    emission: vec3<f32>,
    opacity: f32,
    // How much back-incident light this surface re-emits toward the eye.
    // 0.0 (every existing program) multiplies the transmission term in `fs`
    // to an exact zero, so an existing frame is unchanged to the bit.
    transmission: f32,
};
```

`DEFAULT_SURFACE_WGSL` and `emit::surface_function` each construct one
`SurfaceOut`; both gain a trailing `0.0`.

**(b) `axiom-surface` gains `SurfaceChannel::Transmission`** so an authored
surface can drive it. *Optional for the cloth path* — the hand-written material
program sets the field directly, and the channel enum is only needed once a
field-algebra surface wants to author transmission. Deferring it keeps this
change out of a layer crate. If it is deferred, say so in `emit_lighting`'s
header next to the `metallic`-is-inert note, or it becomes a second inert
channel.

**(c) `scene_wgsl.rs`, `SCENE_WGSL_SUFFIX` — the light loop and one line after
it.** Inside the existing `for` loop, after `let diffuse = …`:

```wgsl
        // Fabric transmission. Directional lights only, and NOT shadowed —
        // the source gathers the light afresh rather than reusing the
        // shadow-attenuated one; occlusion arrives through the surface's own
        // transmission channel instead. `1.0 - step(0.5, lt.v.w)` is the
        // directional-only gate as a multiplier, and a zero colour contributes
        // an exact zero, so a point light adds nothing.
        let dir_only = 1.0 - step(0.5, lt.v.w);
        trans_sum = trans_sum + axiom_cloth_light(
            N, V, L, lt.col.rgb * lt.col.w * dir_only);
```

with `var trans_sum = vec3<f32>(0.0, 0.0, 0.0);` declared before the loop, and
after the loop, immediately before `let emitted = …`:

```wgsl
    // Gated by `diffuse_gate`, not by a new capability bit: an UNLIT surface
    // gathers nothing, and transmission is a gather. `surface.transmission`
    // is 0.0 for every program that does not author it, which makes this an
    // exact identity.
    let transmitted = axiom_cloth_transmitted(
        trans_sum, base.rgb, surface.transmission) * diffuse_gate;
    let emitted = lit + transmitted + surface.emission;
```

Note `L` for a directional light is `normalize(lt.v.xyz)`, the **to-light**
direction, which is what `axiom_cloth_light` expects (three's
`IncidentLight.direction`). `V` is already `normalize(lights.camera.xyz -
in.world_pos)`, which is three's `geometryViewDir`.

**(d) The material program's `axiom_surface` sets**
`transmission: axiom_cloth_transmission(cloth_params, ao)`, and every other
program sets `0.0`.

### 1.6 What "bit-identical when off" actually rests on

- `transmission == 0.0` ⇒ `trans_sum * base.rgb * 0.0` is an exact `(0,0,0)` for
  any finite `trans_sum` and `base`, and `lit + 0.0 == lit` for every finite
  `lit`. (The one exception, `lit == -0.0` becoming `+0.0`, is not reachable
  from a colour channel and is not distinguishable once presented.)
- `trans_sum` itself is only computed, never observed, when the channel is zero.
- The `diffuse_gate` multiply keeps an `Unlit` surface exactly unlit.

Worth confirming with `parity_lighting`'s existing "an existing frame is
unmoved" run rather than by argument alone.

---

## 2. `OVERRIDES` is a table, and its order

The trap is real and I checked it rather than assuming. `OVERRIDES` is an
**ordered array of `[find, replace]` pairs** applied by `String.prototype.replace`
in array order (`shader.js:884`), each replacing the **first** occurrence only.

Reproduced in source order:

| # | `find` | what it does | Axiom counterpart |
|---|---|---|---|
| 0 | `#include <color_fragment>` | deletes three's vertex-colour multiply — vColor is a *mask set* here, not a colour | the material program simply does not multiply by vertex colour |
| 1 | `#include <lights_fragment_end>` | re-emits the chunk **and appends `CLOTH_LIGHT`** | **no counterpart — this is §1** |
| 2 | `#include <roughnessmap_fragment>` | `roughnessFactor = roughness * owORM.g` | `SurfaceOut.roughness` |
| 3 | `#include <metalnessmap_fragment>` | `metalnessFactor = metalness * owORM.b` | `SurfaceOut.metallic` (inert today) |
| 4 | `#include <normal_fragment_maps>` | `normal = owNormalV` | `SurfaceOut.normal` |
| 5 | `#include <aomap_fragment>` | `(owORM.r - 1) * owAoAmt + 1`, applied to **indirect** diffuse (+ clearcoat/sheen/env-spec) | none — see §2.2 |

### 2.1 Is the order load-bearing? Yes, at exactly one entry

Entry 1's *replacement text contains its own `find` string*
(`'#include <lights_fragment_end>\n' + CLOTH_LIGHT`). `String.replace` with a
string pattern does not rescan its own replacement, so entry 1 is safe on its own
— but any later entry whose `find` were `#include <lights_fragment_end>` would
match the re-inserted copy and land in the wrong place. None is, today. That is
one edit away from being false, so:

**The port preserves the source's order and states the invariant** — entry 1 is
the only self-referential replacement, and no other entry's `find` appears in any
replacement text. This port has already had per-surface audio recipes silently
reindexed by treating an ordered table as a set; treating "the lookup looks like a
search, so order cannot matter" as safe is that same mistake.

### 2.2 Five of six entries are the `SurfaceOut` contract restated

That is the finding worth carrying forward. Entries 0, 2, 3 and 4 exist only
because three.js has no channel contract — they are how `owORM`/`owNormalV` reach
the standard lighting. Axiom already *has* that contract, so they port to nothing
at all. Only entry 1 needed a design decision, and only entry 5 needs a home:

**Entry 5 (`aoStrength`) is not mine and is not a `SurfaceOut` channel either.**
It is `mix(1, ao, owAoAmt)` applied to **indirect** diffuse, i.e. to
`ambient_lit` in `scene_wgsl` — the `masks` agent already ported the expression
and flagged the same thing (orchestrator log, cross-cutting item 3). We agree,
independently. Note also that the cloth term is deliberately **not** subject to
it: entry 5 scales `indirectDiffuse` while `CLOTH_LIGHT` adds to `directDiffuse`,
so the fabric applies AO through its own `clamp(owORM.r, 0, 1)` factor at full
strength regardless of `aoStrength`.

---

## 3. The `#define` is not a function — and why hoisting it is still faithful

`OW_CLOTH_LIGHT( IDX )` is a braced textual block, expanded up to three times:

```glsl
#define OW_CLOTH_LIGHT( IDX ) { \
  IncidentLight owCl; \
  getDirectionalLightInfo( directionalLights[ IDX ], owCl ); \
  float owBackLit = max( 0.0, -dot( normal, owCl.direction ) ); \
  float owFwd = max( 0.0, dot( geometryViewDir, -owCl.direction ) ); \
  owTrans += owCl.color * ( owBackLit * ( 0.30 + 0.90 * owFwd * owFwd ) ); \
}
```

Every expansion opens its own scope, declares its own locals, and touches
exactly one outer name, by `+=`, with a value computed only from its own locals.
So the expansion is *exactly* `owTrans = owTrans + f(IDX)` for a pure `f`, and
hoisting `f` into `axiom_cloth_light` preserves both the value and the
evaluation order — **provided** the caller keeps two things, which are the
contract this layer imposes:

1. **Accumulate in light-index order.** Pinned by
   `accumulating_the_lights_out_of_order_is_a_different_float`, which exhibits
   three f32 terms whose forward and reverse sums differ.
2. **Apply the scale once, after the sum.** `owTrans * diffuseColor.rgb * (…)`
   is left-associated and happens outside the macro. Pinned by
   `the_final_scale_is_applied_once_after_the_sum`, which exhibits a triple where
   `(a·b)·s ≠ a·(b·s)` and one where `(a+b+c)·s ≠ a·s + b·s + c·s`. Both triples
   were found by search, not assumed.

`#if NUM_DIR_LIGHTS > 1 / > 2` means fewer lights produce fewer expansions.
That is reproduced **exactly, not approximately**: `axiom_cloth_light` is linear
in `light_color`, so an absent light is a zero colour and contributes a true
`(0,0,0)`, and `x + 0.0 == x`. Pinned by
`an_absent_light_is_a_zero_colour_and_sums_to_an_exact_identity`, and the parity
harness's `cloth_chunk_fs` renders exactly that: three expansions, the third
handed a zero colour.

---

## 4. Semantics worth writing down

### 4.1 `cloth = [0, 1, 0, 0]` must be bit-identical to no cloth at all

The source disables the layer with the **preprocessor**:

```js
if ((p.cloth?.[0] ?? 0) > 0 || (p.cloth?.[1] ?? 1) < 1) defines.OW_CLOTH = '';
```

Axiom has no preprocessor, so the define's own condition is carried as data by
`cloth::enabled` and applied with `select`, which takes the **value**.

This matters concretely, and it is the one place a careless port breaks a frame:
`orm.g = clamp( orm.g + owDown * 0.05, 0, 1 )` is **not** an identity at the
defaults. `owDown` is `smoothstep(0.10, -0.70, n.y)`, which is nonzero on every
downward-facing fragment. A gate written as arithmetic (`* enable`) would leave
the `clamp` in place, which is also not an identity for an out-of-range input. So
the gate is `select(input, computed, on)` at every output.

The albedo half *is* an arithmetic identity — `mix(1.0, 1.0, a)` is exactly
`1.0` for every `a ∈ [0,1]` — but is gated the same way for uniformity.

Pinned by `a_disabled_layer_is_bit_identical_to_no_cloth_at_all`, over 41 world
normals spanning both saturated ends of the ramp and its whole curve, asserting
`assert_eq!` (bit equality) on all four outputs.

Note also `[0, 1, 0.9, 0]` — a fold amount with no transmission and no underside
tint — is **disabled**, because the define does not look at `cloth[2]`. The fold
gate is `enabled(cloth) & (cloth.z > 0.0)`, in that order, not `cloth.z > 0.0`
alone.

### 4.2 The transmission term is unshadowed, on purpose

`CLOTH_LIGHT` calls `getDirectionalLightInfo` afresh rather than reusing
`directLight`, which by `<lights_fragment_end>` has already been multiplied by
the shadow mask (and holds whichever light was added *last* — the moon — which
is the reason the source re-gathers rather than reuses). So a canopy standing in
a cast shadow still transmits. Occlusion comes from the baked cavity/AO instead,
via `clamp(owORM.r, 0, 1)`, which is what stops a canopy inside an arcade from
glowing. Reproduced: the orchestrator's loop edit in §1.5 does **not** multiply
by `atten`.

### 4.3 The fold's basis differs from the source's — a stated divergence

`shader.js:616` does `nShade = normalize( nShade + vec3( tiltC.x, tiltC.y, 0.0 ) )`
where `nShade` is a **view-space** normal (it was built as `owP2V * nP`). Axiom's
`SurfaceOut.normal` is a **tangent-space** normal, perturbed through a
screen-derivative cotangent frame in `fs`.

I ported the arithmetic literally and left the basis to the caller: the function
takes whatever 3-vector normal it is handed. In Axiom it will be handed the
tangent-space normal, so the tilt is applied in the surface plane rather than in
the screen plane.

**What that costs:** for a surface facing the camera the two bases coincide and
the drape is identical. As the surface turns away they diverge, and the source's
version tilts toward *screen* x/y — which means the source's fold ridges rotate
with the camera and Axiom's stay locked to the fabric. Axiom's is arguably the
more correct of the two (a fold is a property of the cloth, not of the viewer),
but it is a difference, and a side-by-side of a rotating awning will show it. It
is not a transcription slip.

### 4.4 Other transcription decisions

- **Every GLSL builtin is written out on both sides** — `clamp`, `mix`,
  `smoothstep`, `dot`, `normalize` — which is `emit_ops`'s existing doctrine
  ("written out rather than handed to the builtin, whose factoring is
  unspecified"). Pinned by `the_wgsl_calls_no_unspecified_builtin`, which strips
  `//` comments before scanning and matches only a *bare* call, so
  `axiom_cloth_smoothstep(` is not confused with the builtin it replaces.
- **`smoothstep` is called descending** (`edge0 = 0.10 > edge1 = -0.70`). Both
  GLSL and WGSL declare the builtin **indeterminate** for `edge0 >= edge1`, so
  calling it would have been a genuine hazard, not just a factoring risk.
- **`normalize` is a division, not a reciprocal-multiply.** GLSL defines it as
  `v / length(v)`. `emit_ops::normalize` deliberately spells the reciprocal
  instead, because it mirrors the field *evaluator*; this is a GLSL
  transcription, and a division turned into a reciprocal-multiply is the single
  defect class this port has found most often (five of ten in `sky/`). Division
  on both sides.
- **`mix` is GLSL's `x·(1-a) + y·a`**, which is *not* `emit_ops::mix`'s
  `a + (b-a)·t`. Different function, different bits; the GLSL one is right here.
- **Left-association preserved** at `0.90 * owFwd * owFwd`,
  `(f0 - 0.5) * z * 0.9`, `tiltC * z * 9.0`, and `owTrans * diffuse * scale`.
- **`-dot(n, l)` negates the scalar; `dot(v, -l)` negates the vector.** Both
  transcribed as written. (They are bit-equal — IEEE negation and rounding are
  sign-symmetric — but transcribing what is there costs nothing and removes the
  need to have been right about that.)
- **Dead computation ported.** `tiltC` is a `vec3` whose `z` is already `0.0`,
  then re-wrapped as `vec3(tiltC.x, tiltC.y, 0.0)`. Kept, with a comment.
- **`cloth[3]` is unused in the source.** Carried as part of the `vec4` and never
  read.
- **`f32` on both sides.** No `f64` anywhere in the reference, so no
  storage-width difference is folded into the tolerance.

---

## 5. What was measured

`cargo test -p axiom-gpu-backend --lib --features offscreen material_shader::cloth`
— 22/22, on a real **Vulkan** adapter (asserted, never skipped).

| | |
|---|---|
| entry points compared | 6 fragment shaders × 24 contexts × 4 lanes = 576 comparisons |
| worst scaled deviation | **1.393e-7** (≈ 1.17 ULP) |
| budget | **1.0e-6** (≈ 8 ULP), i.e. **7.2×** the measurement |
| coverage (`llvm-cov --branch`, no `offscreen`) | 497/497 regions, 42/42 functions, 284/284 lines — **and zero branch regions**, because the non-test Rust is branchless and the tests were rewritten so no `&&` in an `assert!` leaves a dead arm |

The budget is **relative above unit magnitude** — `TOLERANCE * max(|expected|, 1)`
— not absolute. That is not a loosening: one lane this layer produces is a
world-anchored *coordinate* (`axiom_cloth_fold_uv`, ~13.7 at the test's world
positions), whose magnitude is unbounded. An absolute budget on a coordinate is a
budget that silently tightens near the world origin and loosens far from it. The
`max(_, 1)` floor keeps it absolute for the channel-valued lanes, which live in
`0..=1`.

The whole 1.17 ULP is one contracted multiply-add: the GPU fuses
`world_pos.x + world_pos.z * 0.63` into an `fma`. Everything else agrees **bit
for bit**, because every builtin is written out and WGSL requires `+`, `-` and
`*` to be correctly rounded. (The other permitted source, WGSL's 2.5-ULP `/`
against Rust's correctly-rounded one, did not show up above the fma.)

`MEASURED_WORST = 1.4e-7` is asserted, not just documented, so a future adapter
that deviates more fails loudly rather than sliding under the budget.

---

## 6. Handoffs

1. **The `scene_wgsl` / `wgsl_template` edit in §1.5.** Mine to specify, not to
   make. Nothing in `cloth.rs` depends on it having been made.
2. **The three macro-noise fetches stay at the call site.** This layer owns their
   coordinates (`axiom_cloth_fold_uv`, `AXIOM_CLOTH_FOLD_DX/DY`) and the
   arithmetic that consumes them; the texture and sampler belong to whoever binds
   `owMacroTex` — the same binding `macro_variation` uses. Composition:

   ```wgsl
   let cloth_on = axiom_cloth_enabled(cloth);
   let und = axiom_cloth_underside(alb, orm.g, ow_nw, cloth);
   alb = und.xyz;
   orm.g = und.w;
   if (cloth_on & (cloth.z > 0.0)) {
       let f_uv = axiom_cloth_fold_uv(world_pos);
       let f0 = textureSample(macro_tex, macro_smp, f_uv).b;
       let fx = textureSample(macro_tex, macro_smp, f_uv + AXIOM_CLOTH_FOLD_DX).b;
       let fy = textureSample(macro_tex, macro_smp, f_uv + AXIOM_CLOTH_FOLD_DY).b;
       let fold = axiom_cloth_fold(alb, n_shade, cloth, f0, fx, fy);
       alb = fold.albedo;
       n_shade = fold.normal;
   }
   ```

   The `if` is the source's own and exists **only to skip the three fetches** —
   `axiom_cloth_fold` is total and returns its inputs unchanged when off, so
   omitting the `if` is correct but wasteful. The condition is uniform (a
   parameter-buffer value), so it is legal WGSL control flow around a
   `textureSample`.
3. **`aoStrength` (OVERRIDES entry 5) is `masks`'s, and belongs at the lighting
   stage.** Agreeing here independently; see §2.2. It must not be applied to the
   cloth term.
4. **`dead_code` until composition.** Every `pub(crate)` item in this file is
   unused until `axiom_surface` calls them, and `cargo clippy --all-targets -D
   warnings` in CI treats that as an error. No `#[allow]` added, per the
   orchestrator's standing decision (log, cross-cutting item 2).
5. **The hygiene scan is not `cfg(test)`-aware.** `cargo xtask check-architecture`
   rejects `println!`/`eprintln!` *anywhere* in a module, including inside a
   `#[cfg(test)] mod`. I removed mine and folded the measurement into the assert
   message instead. At the time of writing four other layer files still trip it
   (`frames.rs:1121`, `patches.rs:1220`, `pom.rs:1029`, `weathering.rs:2041`) —
   five violations total. Either every layer drops its print, or
   `xtask::hygiene::scan_one` learns to skip `#[cfg(test)]` module bodies. The
   second is the structural fix (the Module Law's own text says these macros are
   rejected *"outside tests"*, so the scan is currently stricter than the law it
   enforces); the first is what unblocks the gate today.
6. **The duplicated GPU parity harness** (orchestrator log, cross-cutting item 1)
   applies here too: `cloth.rs`'s `parity` module is another ~200 lines of
   adapter/render/readback. Deferring to composition is right.

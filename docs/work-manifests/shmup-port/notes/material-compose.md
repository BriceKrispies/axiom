# `material_shader/compose.rs` — the twelve layers, made one shader

The fan-out produced twelve verified fragments of
`C:/dev/Claude-of-Duty/src/materials/shader.js` and **no program**. This file is
the one that makes them one: it concatenates the layers' WGSL and hand-writes
`MAIN_FRAGMENT`, calling each layer in the source's order.

Everything below is either something the composition **does**, something it
**could not do and why**, or an **exact change someone else must make**. The
third category is the one to read first.

---

## 1. What landed

`modules/axiom-gpu-backend/src/material_shader/compose.rs`, plus one line
(`pub(crate) mod compose;`) in `material_shader/mod.rs`. No other file touched.

| item | what it is |
|---|---|
| `material_surface_wgsl(detile: bool) -> String` | the composed program: ten layer constants, then `axiom_lighting_model` + `axiom_surface` |
| `material_program(&MaterialParams) -> MaterialProgram` | the program text **and** its packed parameter block, with the de-tiling gate taken from the same material |
| `MaterialProgram { wgsl, params }` | `params` is `[[u32; 4]; 32]` — `pack()`'s floats as bit patterns, so the pair is `PartialEq`/`Hash`-able as a pipeline identity |
| `OW_DIST` | the one place the missing view distance is bound (see §5) |

**19 tests, all green**, on a real Vulkan adapter:

```
CARGO_TARGET_DIR=.../compose cargo test -p axiom-gpu-backend --lib \
  --features offscreen material_shader::compose
test result: ok. 19 passed; 0 failed
```

`compose.rs` carries `#![cfg(any(test, target_arch = "wasm32", feature =
"offscreen"))]` because `patches.rs` does: `PATCHES_WGSL` does not exist on a
default-feature native build, and dropping the layer to avoid the gate would be
exactly the failure this file exists to prevent.

---

## 2. The composition order — it *is* the specification

Float arithmetic is not associative, so "which layer runs first" is not style. A
layer applied out of order compiles, renders, and looks entirely plausible. The
order is `MAIN_FRAGMENT`'s, line for line; the WGSL carries `shader.js:NNN`
comments so the two diff by eye.

| `shader.js` | layer | what runs |
|---|---|---|
| 255-267 | — | `owDist`, `owFaceDir`, `owNw`, `owP`, `owNp` |
| 323-336 | `uv_mode` + `frames` | the projection frame and its uv |
| 338-351 | `pom` | derivatives, then the parallax march |
| 353-356 | — + `tint_wear` | the three base fetches; `owNormalAmp` |
| 358-369 | `detile` | the second, rotated sample **(gated textually)** |
| 371-378 | `detail` | the micro normal, folded into the base sample |
| 379-383 | `detile` | fold into sample two, the mask, the height blend |
| 385-393 | `detail` | micro albedo, cavity roughness, `owHeightS` |
| 396-449 | `macro_variation` | two bands, hue, roughness, relief |
| 451-490 | `patches` | repair patches on vertical faces |
| 492-565 | `weathering` | dust, rain runoff, ground splash, dust wedge |
| 567-596 | `masks` | cavity grime + the vertex-colour masks |
| 598-618 | `cloth` | underside, fold |
| 620-628 | `tint_wear` | tint, roughness remap, the channel assignment |
| 650-666 | `cloth` | the transmission channel |

### `mac1` / `mac2` / `up_face` are threaded, never recomputed

`axiom_macro_variation` returns them and five later calls read them
(`patches`' lattice wander takes `mv.mac2.rg`; the weathering stack takes both;
`masks` takes both). Recomputing would double four texture fetches and fork the
macro uv into two definitions able to drift.

### `CLOTH_WGSL` is **not** concatenated

`scene_shader` already splices it into every scene shader, because the lighting
stage calls `axiom_cloth_light` and `axiom_cloth_transmitted`. A second copy is a
duplicate definition and does not compile. A test asserts the composed program
defines none of the cloth functions and that exactly one definition reaches the
spliced scene shader.

### `owVert` / `owSAxis` / `owHash11` were **not** hoisted

The `patches` agent recommended a shared prologue and the expressions are
byte-identical, so it would be exact. It was not done, for one reason: both
copies live *inside* `axiom_patch_apply` and `ow_weather_stack` and neither takes
them as parameters, so hoisting means **changing two layer files' signatures**,
which this file may not do. The cost is a handful of duplicated ALU ops and two
identical WGSL function bodies (`axiom_patch_hash11` / `ow_hash11`) that the
shader compiler is free to merge. If the orchestrator wants it hoisted, it is a
signature change in two layers plus one line here — a rename, not a
re-derivation.

---

## 3. `owRoughP` is **not** slot 11 — the one packing trap

`extendMaterial` (`shader.js:836-843`) packs

```
owRoughP = ( roughness[0], roughness[1], DETILE, roughness[2] )
```

while `params.rs` slot 11 is `( scale, offset, minimum, ao_strength )`. Two
consumers read two *different* lanes of `owRoughP`: `tint_wear`'s
`axiom_mat_roughness_remap` reads `.w` as the per-surface floor, and the de-tile
height blend reads `.z` as the blend amount. So the composition rebuilds it:

```wgsl
let rough_p = vec4<f32>(p_rough.x, p_rough.y, p_misc.w, p_rough.z);
```

Pinned by `the_rough_vector_is_rebuilt_because_it_is_not_slot_eleven`. Without
it the roughness floor would read `ao_strength` and de-tiling would read the
floor — both would render plausibly.

Two other values are **derived in the shader** because `params.rs` packs the
authored value, not the derived one:

* `owTile.xy` = `mesh ? scale : 1/scale` (`extendMaterial:793`). The reciprocal
  in f32 is exactly the CPU's, so nothing moves by doing it per-fragment.
* `owDetailP.x` = `mesh || !(dw > 0) || scale < 0.3 ? detail[0] : max(1.2, scale/dw)`
  (`extendMaterial:805-810`), written as the `select` chain that propagates a NaN
  the way `Math.max` does and `max` does not — the same shape `detail.rs`'s
  `detail_tiles` CPU reference has.

Slot 13 (`no_grad`) is deliberately **unread**: `OW_NOGRAD` swaps every
`textureGrad` for `texture2D`, which is a fourth permutation for a debugging
flag. `the_composition_reads_only_the_slots_the_map_defines` pins that the
composition reads slots 0-12 and 14-18 and nothing else.

---

## 4. De-tiling is gated **structurally**, and that is why the deliverable is a
function and not a `&str`

The brief required it, and the `detile` agent's measurement is the reason: a
runtime `t = 0` through the height blend is **not** bit-identical to omitting the
block — 1 ULP on 17.2% of operands. The source gates it with a preprocessor
define, so the composition gates it with text: the two `#ifdef OW_DETILE` chunks
are separate constants spliced into a shared body, so the other ~200 lines stay
singular and cannot drift between permutations.

**The cost, stated plainly:** a program text with two shapes cannot be a
`pub(crate) const MATERIAL_SURFACE_WGSL: &str`. Rust has no stable const string
concatenation that does not require a `while` loop, and a `while` loop is Rust
control flow the Branchless Law bans — so the const form is not merely
inconvenient here, it is unavailable. `material_surface_wgsl(detile)` is the
deliverable instead.

The permutation is currently **two** programs, and it interacts with the pipeline
cache — see §6.2.

Everything else that *looks* like it needs a permutation does not, and each was
checked rather than assumed:

| source `#ifdef` | why no permutation |
|---|---|
| `OW_PARALLAX` (`parallax > 0`) | `axiom_pom` returns `uv` unchanged for `depth <= 0.0` — an exact identity, which is the same test the define makes |
| `OW_PATCH` (`patch[0] > 0`) | coverage 0 makes `has = step(1.0, r0)` with `r0 = fract(..) < 1`, an exact zero mask |
| `OW_CLOTH` | `axiom_cloth_enabled` is a value gate inside the layer, and `select` takes the untouched value |
| `OW_MESH_UV` | planar and mesh differ only in the frame, which is arithmetic; one `select` per component |
| `OW_VCOL_MASKS` | `select` on the value in `masks`, `vcol_masks = 0` in `weathering` — both proven bit-identical by their own layers |
| `OW_WEATHER` | **almost**: with all three amounts zero every term is an exact identity *except* `normalize(mix(n, flat, 0.0))`, which renormalises an already-unit vector. Sub-ULP, and the default has weathering on. Noted, not hidden. |
| `OW_TRIPLANAR` | genuinely a permutation, and **not composed** — see §5 |
| `OW_NOGRAD` | genuinely a permutation, and not composed (debug flag) |

---

## 5. What could not be threaded, and exactly why

Each of these is a **contract gap**, not a shortcut. Each has a one-line fix
listed in §6.

### 5.1 The view distance — `owDist`

`float owDist = length( vViewPosition );`. `SurfaceIn::view_dir` is
`normalize(camera - world_pos)`, so the length is gone and cannot be recovered.
`OW_DIST` binds `0.0`, which makes both distance fades evaluate to exactly `1.0`:

* `axiom_pom_fade(0, 6, 14)` = `1.0`
* `axiom_detail_fade(16, 0)` = `1.0`

That is the **near-field** behaviour, so POM and the detail layer do their full
work and are observable — what is missing is the fade to nothing at range, which
is a real visual regression at distance (shimmer instead of a clean fade) and a
real cost (POM marches at full layer count everywhere).

### 5.2 `gl_FrontFacing` — `owFaceDir`

Bound to `+1`. `SurfaceIn` carries no facing lane, so a back face reads its own
normal rather than the flipped one. Affects every `owNw`/`owNp` consumer:
triplanar axis choice, `owVert`, `owDown`, the dust `up` term.

### 5.3 The vertex-colour masks — `vColor`

`SurfaceIn` has no `vertex_color` lane, and it cannot be recovered from
`in.albedo`: Axiom pre-multiplies the vertex/instance colour into the albedo,
whereas the source *overrides* `<color_fragment>` precisely so the lane stays a
**mask** and is never multiplied in. The composition passes
`vec3<f32>(0.0, 0.0, 0.0)` — which is exactly what the default
`vertexMasks: false` means — and threads the flag from slot 12 so the plumbing is
right the day the lane exists. Pinned by
`the_absent_vertex_colour_lane_is_an_explicit_zero`, so it cannot rot into an
unremarked zero.

Until then, setting `vertex_masks: true` on a material produces a *defined but
inert* wear/grime/AO layer. That is the one place where the composition's honest
answer is still a trap for an author, and it is the strongest argument for adding
the lane.

### 5.4 View space — the shading normal

The source's `nShade` is a **view-space** normal and three sections perturb it
there. The composition has no `mat3( viewMatrix )`, so it builds the shading
normal in **world** space and passes the identity where the macro layer wants the
view rotation. That substitution is exact for two of the three:

* **macro relief** — `normalize(nShade + mat3(viewMatrix) * tiltW)`: a rotation
  commutes with a sum and with `normalize`, so the world-space form is the same
  vector rotated, i.e. the same normal.
* **weathering's dust and wedge softening** — `normalize(mix(nShade,
  normalize(owP2V * owNp), t))`: both operands are the same rotation of their
  world-space counterparts, and `mix` is linear.

The **cloth fold** is the exception and the one knowingly-inexact substitution in
the file: it adds a *view-space* `xy` offset (`nShade + vec3(tiltC.x, tiltC.y,
0.0)`), which has no world-space equivalent without the view matrix. It is
applied in world space. The fold's **albedo** term is exact; its normal tilt is
not the source's. Flagged in the code at the site.

### 5.5 AO — `owORM.r`

`SurfaceOut` has no AO lane. **`aoStrength` is deliberately not applied in
`axiom_surface`**, exactly as the brief says: the `masks` layer found it belongs
at the lighting stage (`shader.js:678`, `( owORM.r - 1.0 ) * owAoAmt + 1.0`, a
lerp toward 1 applied to `reflectedLight.indirectDiffuse`). So:

* `axiom_masks_ambient_occlusion` is defined and **uncalled** — correct, and it
  should stay uncalled until the lighting stage can host it;
* every layer's AO work (detail's cavity darkening, weathering's splash, masks'
  `vColor.b` term) is computed and reaches exactly **one** consumer,
  `axiom_cloth_transmission(cloth, orm.r)`, which does read it;
* otherwise it is dropped. Slot 11's `ao_strength` lane is packed and unread.

This is not worked around. It needs a `SurfaceOut.ao` lane plus the lerp in the
lighting stage — the same shape as the `transmission` lane cloth already got.

### 5.6 Triplanar

`OW_TRIPLANAR` is a third *permutation*, not a runtime mode: nine implicit-LOD
fetches, its own frame set, and its own detail arm
(`axiom_detail_blend_normal_projected` on the dominant plane). Planar and mesh
differ only in the frame — pure arithmetic — so those two share one program via
`select`, and a test proves they render differently. Triplanar is **not
composed**. Cost: a second `axiom_surface` body of ~60 lines, and the uv-mode
layer's `axiom_uv_triplanar_weights` / `axiom_uv_triplanar_detail_axis` plus the
detail layer's projected blend stay unreached.

### 5.7 The instance colour

`diffuseColor.rgb *= owAlbedo.rgb` in the source, where `diffuseColor` is the
material colour. Axiom's `in.albedo` is *already* `albedo_tex × vertex ×
instance`, and the composition takes its own `textureSampleGrad` of `albedo_tex`
through the projected uv — so multiplying by `in.albedo.rgb` would double-count
the texture. `base_color` is the composed albedo and the **instance colour tint
is not applied**. `in.albedo.w` is used as the material opacity, which is
correct.

---

## 6. What the orchestrator must do

### 6.1 `surface_program/cache.rs` names a symbol that cannot exist — **blocking**

At the time of writing, the working tree's `cache.rs` has

```rust
fragment: String::from(crate::material_shader::compose::MATERIAL_SURFACE_WGSL),
```

and that is the **only** compile error in the crate. `MATERIAL_SURFACE_WGSL`
cannot be a `&str` (§4). The one-line replacement, which also picks the right
permutation from the material the caller already has in hand:

```rust
fragment: crate::material_shader::compose::material_program(&params).wgsl,
```

or, if the caller wants to keep `param_bytes` where it is:

```rust
fragment: crate::material_shader::compose::material_surface_wgsl(
    axiom_surface::detile_is_on(&params),   // or inline: params.detile > 0.0 && params.uv_mode != UvMode::Triplanar
),
```

`material_program` is the better seam: it derives the gate and packs the block
from **one** `MaterialParams`, so the emitted text and the numbers behind it can
never disagree about whether the de-tile block exists.

### 6.2 `program_id` must carry the de-tiling gate

`cache.rs`'s comment says the digest "carries the KIND but not the parameter
values — so every runtime material in a scene is one program and one pipeline,
differing only in the bytes below." With the structural gate that is no longer
true: two materials differing **only** in `detile` now emit different fragment
text under the **same** `program_id`, which is a silent cache collision — whichever
compiled first wins and the other renders with the wrong shader.

Three ways out, in order of preference:

1. fold the gate into the program digest (one bit);
2. hash the emitted text into the id;
3. drop the structural gate and accept the 1-ULP difference — **not recommended**,
   the brief asked for the opposite and the measurement is real.

### 6.3 `SurfaceIn` needs three lanes

In descending order of what they cost to omit:

| lane | why | what it fixes |
|---|---|---|
| `view_distance: f32` (or the un-normalised camera vector) | `length(vViewPosition)` | POM's `parallaxFade`, detail's fade — both currently pinned at full strength (§5.1) |
| `vertex_color: vec3<f32>` | the source's masks are a *mask*, not a tint | the whole `OW_VCOL_MASKS` path, currently defined and inert (§5.3) |
| `front_facing: f32` | `owFaceDir` | back faces (§5.2) |

`local_space` needs a fourth — an object→world rotation — before an
object-space projection can produce a world-space shading normal. Until then
`local_space: true` builds the frame in object space and treats it as world.

### 6.4 `SurfaceOut` needs an AO lane

Plus `( ao - 1.0 ) * ao_strength + 1.0` applied to the ambient term in
`SCENE_WGSL_SUFFIX`, which is what `axiom_masks_ambient_occlusion` already is.
Without it, five layers' AO work reaches only the cloth transmission scalar
(§5.5).

### 6.5 `SurfaceOut.normal` is currently **dead** in the scene shader

This is the sharpest finding in the file and it is not about the composition at
all. In `SCENE_WGSL_SUFFIX`:

```wgsl
let nmap = select(surface.normal, textureSample(normal_tex, ...).xyz * 2.0 - 1.0,
                  (caps & CAP_NORMALMAP) != 0u);
...
let mapped = normalize(tangent * (nmap.x * inv_max) + ... );
let N      = select(geo_n, mapped, (caps & CAP_NORMALMAP) != 0u);
```

The two `select`s take **opposite** arms of the same condition:

* capability **off** → `nmap = surface.normal`, but `N = geo_n`, so `mapped` (and
  with it `surface.normal`) is discarded;
* capability **on** → `nmap` is the raw `normal_tex` sample, so `surface.normal`
  is discarded.

There is no path on which a surface program's authored normal reaches the
lighting. Every layer's normal work — the detail UDN blend, the de-tile blend,
macro relief, the weathering softening, the cloth fold — is computed and thrown
away. The composition still produces it (and the GPU test proves it varies), but
it is currently write-only.

That also settles the space question the `tint_wear` agent raised: the fix is not
to make the composition emit a *tangent*-space normal for Axiom's screen-space
cotangent frame — that frame is built from `in.uv`, which is **not** the projected
uv the material samples through, so it is the wrong frame. `SurfaceOut.normal`
should be **world space** (Axiom's lighting is world-space: `N`, `L` and `V` all
are), and `fs` should take it when a program authors one.

### 6.6 The alpha cutout is owned twice

`scene_wgsl.rs` discards at `albedo.a < 0.5` **before** the surface program runs,
gated on `CAP_ALPHAMASK`. The source cuts at three's `alphaTest` on the composed
albedo, after. `axiom_mat_alpha_cut` is therefore defined and uncalled — a
surface program returns a `SurfaceOut` and cannot `discard`. Someone has to
decide which cutout owns it.

### 6.7 Group 0 binds one detail map, and the source wants two

`owDetailNrm` (detail normal) and `owDetailTex` (detail albedo + height) are two
uniforms in the source; group 0 has `material_detail_tex`. Both fetches take it.
That is the **source's own fallback** — `owDetailTex: shared.detailAlbedo ??
shared.detailNormal` (`extendMaterial:812`) — not a substitution invented here,
so it is correct-by-the-source when no detail albedo is authored. A second
binding is needed before one can be.

There is also no dedicated sampler for `material_orm_tex` / `material_detail_tex`
/ `material_macro_tex`; the composition uses `albedo_sampler`, as `scene_wgsl`'s
own comment says to. The macro map in particular needs **repeat** addressing or
every world-anchored layer clamps at the tile edge.

### 6.8 A shared GPU harness

Every layer author reported it and this file makes thirteen: `ParityGpu` is
`pub(super)` to `surface_program`, so `material_shader` carries its own ~250-line
adapter/upload/render/readback copy. One `material_shader/parity_gpu.rs` should
absorb them now the fan-out has closed. This file's copy is a reasonable donor —
it binds five textures, two samplers and a uniform, which is the widest of them.

---

## 7. Which WGSL functions the composition reaches, and which it does not

`every_layer_entry_point_is_called_from_the_composition` pins twenty-nine calls
by name **inside the body of `axiom_surface`**, not merely somewhere in the text.
`switching_any_one_layer_on_changes_what_is_rendered` then proves at runtime, one
layer at a time, that the call reaches `SurfaceOut` — the text test catches a
dropped layer, the render test catches a layer threaded with the wrong argument
or assigned to a local nothing reads.

**Not reached, each for a stated reason:**

| function | layer | why |
|---|---|---|
| `axiom_detail` | detail | the aggregate un-de-tiled convenience. The composition calls its seven *parts* instead, because de-tiling needs `dn` in scope between the normal blend and the height blend — which is exactly the interleave the `detile` agent's note prescribes. |
| `axiom_uv_axis_sign`, `axiom_uv_axis_project`, `axiom_uv_axis`, `axiom_uv_planar` | uv_mode | `frames`' `owAxisFrame` computes the same uv *and* the TBN in one call, and `MAIN_FRAGMENT` calls `owAxisFrame`. The uv-only duplicates are redundant with it. `axiom_uv_dominant_axis` — the one piece `frames` deliberately does not own — **is** called. |
| `axiom_uv_triplanar_weights`, `axiom_uv_triplanar_detail_axis`, `axiom_detail_blend_normal_projected` | uv_mode, detail | triplanar is not composed (§5.6) |
| `owTangentFrame` (4-arg) | frames | called through `owTangentFrameScreen`, its own wrapper |
| `axiom_mat_alpha_cut` | tint_wear | a surface program cannot `discard` (§6.6) |
| `axiom_mat_wear_albedo`, `axiom_mat_wear_orm` | tint_wear | the `masks` layer already applies `shader.js:588-590` inline; calling both would apply the wear twice |
| `axiom_masks_ambient_occlusion` | masks | belongs at the lighting stage (§5.5) |
| `axiom_cloth_light`, `axiom_cloth_transmitted` | cloth | called by `SCENE_WGSL_SUFFIX`'s light loop, not by the surface program — which is why `CLOTH_WGSL` is spliced by `scene_shader` |
| `axiom_detile_*` | detile | reached only in the de-tiling permutation, which the tests exercise separately |

---

## 8. The tests, and what each would catch

| test | the defect it catches |
|---|---|
| `every_layer_is_concatenated_once_and_before_the_composition` | a layer omitted, duplicated, or spliced after its use |
| `every_layer_entry_point_is_called_from_the_composition` | a layer silently dropped from the composition |
| `switching_any_one_layer_on_changes_what_is_rendered` | a layer *called* but wired to the wrong argument, or whose result nothing downstream reads — eight layers plus cloth, one parameter each, on a real GPU |
| `the_de_tiling_permutation_changes_what_is_rendered` | the same, for the layer that is a permutation rather than a uniform |
| `de_tiling_is_absent_from_the_text_when_it_is_off` | the structural gate degrading into a runtime zero |
| `the_cloth_layer_is_called_but_never_redefined` | the duplicate-definition compile failure, and its inverse |
| `the_composition_reads_only_the_slots_the_map_defines` | a slot read at the wrong index — the failure that re-reads someone else's parameter and still renders |
| `the_rough_vector_is_rebuilt_because_it_is_not_slot_eleven` | the `owRoughP` lane trap (§3) |
| `all_seven_surface_out_channels_are_written` | a channel left at its zero default |
| `the_missing_view_distance_is_bound_where_a_reader_can_find_it` | `ow_dist` going un-fixed silently when the lane arrives |
| `the_absent_vertex_colour_lane_is_an_explicit_zero` | the inert-mask trap (§5.3) rotting into an unremarked zero |
| `the_composed_program_compiles_inside_the_real_scene_shader` | anything the text tests cannot see: a uniformity violation on an implicit-LOD sample, a type error, a name collision with `scene_wgsl` |
| `the_composed_surface_renders_a_non_constant_image` | a composition that returns a plausible flat colour — checked per channel, plus a finiteness check so a NaN cannot read as "varies" |
| `the_mesh_uv_mode_renders_differently_from_the_planar_one` | the frame `select` not reaching the sample |

The GPU module **asserts** an adapter was acquired rather than skipping.

One measurement worth keeping: the render tests' sample walk spans about three
2.6 m patch cells and five vertically, and the stride is chosen for that. An
earlier walk confined to roughly one cell reported the (working) repair-patch
layer as moving three of thirty-two samples — indistinguishable from a dead
layer. A sparse world-anchored layer needs a walk wider than its own period, or
the test lies in the safe direction.

---

## 9. Laws

* **Branchless Law** — the non-test Rust is `usize::from(bool)` table indexing,
  `.map`, `.concat`. Zero `if`/`match`/`for`/`&&`/`?`. The WGSL keeps the loops
  it is named for (POM's march), which is shader text and therefore data.
* **Coverage Law** — every non-test item is exercised: the four public items, the
  five WGSL chunk constants, both `MaterialProgram` derives (`Debug` is formatted
  explicitly rather than relied on via `assert_eq!`, which only formats on
  failure). `Eq` was **not** derived, for the same reason — its
  `assert_receiver_is_total_eq` is never called.
* **Module Law** — no new dependency; the composition names only sibling layers
  and `axiom_surface` (already a dependency, and now the home of
  `MaterialParams`).
* No `println!` anywhere, tests included.


---

## 10. Addendum — the ornament gate (superseding §4's "two programs")

Landed later, in the same file. This note's §4 says *"the permutation is
currently **two** programs"* and §1's table gives
`material_surface_wgsl(detile: bool)`. Both are superseded: there is a second
structural gate, and the signature is
`material_surface_wgsl(detile: bool, ornament: Ornament)`.

**Why.** The composition put all twelve layers in every fragment
unconditionally, and the app that pays for it is fill-rate bound — at render
scale 1.0 the frame costs 29.2 ms, and cutting the backbuffer 1280x720 → 640x360
cut it 4.2x, almost exactly proportional to pixel count, with draws, instances
and triangles identical across seven camera views.

**The split is the source's.** `apps/shmup/src/core/fidelity.js`'s lean tier
drops exactly `OW_PARALLAX`, `OW_DETILE`, `OW_WEATHER`, `OW_PATCH`, `OW_CLOTH`
and `OW_MACRO_RELIEF` (`shader.js:883`), and deliberately leaves the projection,
masking and channel defines outside that `if (!LEAN)` — dropping *those* "does
not simplify the material, it makes it sample the wrong thing."

**One global switch, not per material.** `Ornament::of` reads
`axiom_host::RenderCapability::SurfaceOrnament` off the profile that prepares the
catalog, before any program is generated, so it never reaches a surface digest
and `SurfaceKind::code` stays structural. A per-material gate would cut more per
fragment but multiply permutations, and the same `fidelity.js` measures cold boot
as `(lit programs) x (~100 KB of translated shader each)` — 101 programs at ~26 s
against lean's 43 at ~14.8 s.

**The permutation count is three, not four.** De-tiling is one of the six layers
lean drops, so `material_surface_wgsl` multiplies the two gates together and the
`(lean, de-tiled)` combination is unrepresentable rather than merely unused. The
emitted shapes are `{full·detile-off, full·detile-on, lean}` — a `+1` on §4's
two, pinned by `the_ornament_gate_adds_one_program_shape_not_a_second_axis`.

**How the reduction is visible.** `SurfaceProgramSource::ornament_reduced` is a
fact about the text, kept beside it; `SurfaceProgramCatalog::degradations` raises
`axiom_host::FrameFeature::SurfaceOrnament` only for a frame that actually draws
one of those programs — keyed on the frame exactly as the miss report is, because
a standing per-backend flag would fire on every frame in the engine.

# material maps — the five-texture contract, and the slice that collapses a lane

Slice: the engine contract change `notes/materials-upload.md` specified after its
bake produced four maps the engine had no way to accept. Spine tier
(`crates/axiom-host`, `modules/axiom`, `modules/axiom-gpu-backend`).

Written under `12-final-wave-brief.md`: **nothing here has been compiled, run or
committed.** Every tolerance and every "this holds" below is unverified.

## The two defects

1. `axiom_host::MaterialTexture` carried **albedo and nothing else**. The runtime
   material shader binds five maps (albedo, normal, `orm_height`, `detail`,
   `macro_field`), and three of its twelve composed layers — parallax occlusion,
   de-tiling, micro detail — were inert because no app could supply them.
   `scene_renderer.rs` bound neutral 1x1s at 4, 5 and 6 with a comment saying an
   app would supply real ones "at stage 5"; this is stage 5.

2. Separately and worse: the normal map travelled in a **second slice parallel to
   the material set** (`normals: &[(u64, u32, u32, Vec<u8>)]`), and
   `live_gpu_binding.rs:391` passed `&[]`. The live browser arm has had no
   normal-map lane at all for as long as the lane existed, and nothing caught it,
   because nothing could: the two slices were independent, so forgetting one was
   type-correct.

Defect 2 is the argument for the shape of the fix to defect 1. Four *more*
parallel slices would have been five things to forget instead of one.

## What landed

**`crates/axiom-host/src/material_texture.rs`** — a new public value type
`MapPixels { width, height, pixels }` (extent + row-major RGBA8, no material id,
no sampling mode — a map has neither), and four `Option<MapPixels>` fields on
`MaterialTexture`: `normal`, `orm_height`, `detail`, `macro_field`, with
`with_*(Option<MapPixels>)` setters and `Option<&MapPixels>` accessors.

The lowest correct layer by the type's own module doc: "the one place `axiom`,
`axiom-windowing`, `axiom-gpu-backend` and `axiom-canvas2d-backend` can all name a
type." Every existing producer builds through `MaterialTexture::new`, and the four
fields default to `None`, so this is purely additive at every call site.

The setters take an `Option` rather than a `MapPixels`. Their one engine caller
resolves four ids that may each be absent, and a set-if-present combinator around
a by-value builder would move the whole carrier four times per material. It is
also branchless, which the alternative is not.

**`modules/axiom/src/material.rs`** — four `u64` ids (`normal_texture`,
`orm_texture`, `detail_texture`, `macro_texture`) with `with_*` builders and
accessors, resolved through the **existing** `custom_textures` store that
`RunningApp::add_texture_data` already fills. No new store: a map is RGBA8 pixels
registered at runtime, which is exactly what that store holds, and *which slot*
is a property of the material, not of the pixels. Scalars, so `Material` stays
`Copy` — pinned by `a_material_carrying_every_map_is_still_copy`.

**`modules/axiom/src/app/resources.rs`** — `material_textures()` fills the four
slots through one private `map_pixels(id) -> Option<MapPixels>`. Id `0` (the
cleared value, which no registration ever issues) and a stale id both resolve to
`None`, because an unresolvable id *is* a missing map and a missing map is what
the backend's neutral is for. The albedo keeps its own lookup rather than routing
through `map_pixels`, which would clone a multi-megabyte payload twice per
material at bind.

**`modules/axiom-gpu-backend/src/scene_renderer.rs`** — `SceneRenderer::new`
**drops** its `normals` parameter and reads all five maps off the carrier, through
one `map_or_neutral(Option<&MapPixels>, &neutral) -> (u32, u32, &[u8])`. The whole
fallback rule for all four slots lives in that one function on purpose: four
slots each writing their own `unwrap_or` is four chances to pick a different
default, and the difference between "authored nothing" and "authored black" is a
frame with the macro layer subtracted from it. **The neutral constants did not
move.** `offscreen.rs` and `live_gpu_binding.rs` shrink by one argument each.

## The hard constraint, and how it is proved

*A material that supplies no extra maps must render byte-identical to today.*

`scene_renderer.rs`'s new `map_tests` module (native + `offscreen`, the
`gbuffer.rs` precedent):

- `a_material_with_no_maps_matches_one_that_binds_the_neutrals_byte_for_byte` —
  renders the same lit quad twice, once with no maps and once with all four set
  explicitly to the backend's neutral bytes, and asserts **0 differing bytes of
  16384**. The expected neutrals are written out in the test rather than read from
  the implementation, so the two can disagree. Expected tolerance: exact, zero
  bytes. **Unverified — not run this wave.**
- `an_authored_normal_map_changes_the_shaded_frame` — the byte-identity test
  passes trivially if the maps are silently dropped, which is exactly defect 2.
  This is the assertion that would have caught it. Expected: > 0 differing bytes.
  **Unverified.**
- `map_or_neutral_takes_the_authored_map_and_falls_back_to_the_neutral` — both
  arms, no GPU.

The quad carries **varying uv**, which is load-bearing: the main pass builds its
cotangent frame from screen-space uv derivatives, and a quad whose four corners
share one uv has a degenerate frame that `scene_wgsl` deliberately resolves to the
geometric normal — on which a normal map changes nothing. A zero-uv quad would
have "proved" the lane works by proving nothing.

Risk if this fails on the orchestrator's adapter: the likely cause is
`an_authored_normal_map_changes_the_shaded_frame`, not the identity test — the
tilt must survive the cotangent frame at this quad's orientation. If it reports 0,
widen the tilt or angle the quad; do not loosen the identity test.

## The second gap: binding 5 is one channel short of what the shader reads

`materials-upload.md` found it and left the decision to whoever owns
`material_shader/`. **The decision is: pack `(normal.x, normal.y, micro_albedo,
height)`. Do not add binding 7.**

Binding 5 is documented `(normal.rgb, height.a)`, but the source samples **five**
scalars through **two** textures: `detailNormal.xyz`, `detailAlbedo.r` (micro
albedo) and `detailAlbedo.a` (micro height). Under the documented packing
`compose.rs`'s `d_tex.r` reads the *normal's x*, which on a near-flat detail
normal is ~0.5, so `(dTex.r - 0.5) * 1.25` contributes nothing and half the micro
layer stays dead even once a real tile is bound.

Why the packing and not a seventh binding:

- **It is lossless for what is actually read.** Both consumers of `dn` in
  `compose.rs` are UDN blends — `axiom_detail_blend_normal` (detail.rs:129) and
  `axiom_detile_fold_detail_normal` (detile.rs:157) — and both sum the tangent
  `xy` and keep the **base** normal's z. `dn.z` is never read on this path. Four
  channels carry four consumed scalars exactly.
- A seventh binding costs a fifth map slot on the carrier, a `scene_wgsl.rs`
  change and another texture against a WebGL2 downlevel binding budget, to buy a
  channel the packing already fits.

**It is three coordinated lines, not one**, and the third is the one the notes
missed. All three must land in the same commit, owned by `material_shader/`:

1. `modules/axiom-gpu-backend/src/material_shader/compose.rs:319` — `dn` must come
   from `.xy` with z reconstructed, not from `.xyz`:
   `let dn_xy = d_raw.xy * 2.0 - 1.0; let dn = vec3<f32>(dn_xy, sqrt(max(0.0, 1.0 - dot(dn_xy, dn_xy))));`
   The reconstruction is not needed by today's two UDN consumers; it is needed the
   moment `axiom_detail_blend_normal_projected` (the triplanar arm, detail.rs:142)
   is wired, which *does* read the full vector.
2. `modules/axiom-gpu-backend/src/material_shader/compose.rs:347` — the micro
   albedo must read the packed blue, not red:
   `axiom_detail_albedo(alb.rgb, vec4<f32>(d_tex.b, d_tex.g, d_tex.b, d_tex.a), micro, detail_p.z, det_fade)`.
   `axiom_detail_micro` keeps reading `.a` and is unaffected.
3. `modules/axiom-gpu-backend/src/scene_renderer.rs` — `neutral_detail` must go
   from `(1, 1, [128, 128, 255, 0])` to `(1, 1, [128, 128, 128, 0])`. With `.b`
   read as the micro albedo, `255` decodes to `1.0` and `(1.0 - 0.5) * 1.25 =
   0.625` — a non-identity that brightens **every un-mapped material** by 62.5% of
   its detail strength. `128` is the identity. Landing (1) and (2) without (3)
   breaks the byte-identity test above, which is precisely what that test is for.

`apps/shmup/src/materials/upload.rs` writes the *currently documented* packing and
must be repacked in the same change.

## Deferrals, with their expiry conditions

- **The packing fix is not applied here.** `compose.rs` and `scene_wgsl.rs` are
  out of this slice's write scope, and applying only the `neutral_detail` byte
  would break byte-identity on its own. Expires when `material_shader/`'s owner
  lands the three lines above; the file that must change is
  `modules/axiom-gpu-backend/src/material_shader/compose.rs`.
- **No app authors a map yet.** The carrier, the ids, the resolution and the bind
  all exist; `apps/shmup`'s `bake_library` already produces all four payloads.
  Expires the moment `apps/shmup/src/scene/app.rs` calls
  `Material::with_normal_texture` and friends — at which point the shmup's
  parallax, de-tiling and micro layers become live for the first time.
- **`Canvas2dBackendApi` ignores the four maps.** It is a CPU rasteriser with no
  material shader; the maps are GPU-path data. Expires only if the Canvas 2D
  backend grows normal mapping, which is not planned.

# Slice 5 — the gun's materials and the hands

**New file:** `apps/shmup/src/scene/wiring/weapon_look.rs`
**Touched:** `apps/shmup/src/scene/wiring/mod.rs` (one `pub mod` line)
**Not touched:** `scene/app.rs`, `scene/game.rs`, `scene/mod.rs`, `lib.rs`,
`Cargo.toml`, `app.toml`, and everything under `apps/shmup/src/weapons/` — the
ported code needed no edits, only callers.

Nothing was built, checked, tested or linted, per the wave rules.

---

## What was wired

### 1. `weapons/materials.rs` → the rifle

`install_rifle` bound `Material::lit(bucket_color(bucket))`.
`crate::viewer::bucket_color` says what it is in its own doc comment — *"the
viewer's own reading of them, not the game's material system"* — nine hand-picked
greys written for a turntable that had no way to bind a shader graph. That is
why the gun is untextured.

`WeaponLook` replaces it, and it is **the level's own path with the weapon table
substituted for the palette**:

| step | street (`MaterialLook`) | gun (`WeaponLook`) |
|---|---|---|
| the table | `Palette::ALL` → opts | `WEAPON_MATERIALS` → opts |
| the merge | `MaterialSystem::get` | *the same function* |
| the surface | `engine_params` + `runtime_material` | *the same functions* |
| the texture | `bake_albedo_maps` | `TextureSet::bake_at(size, false, false)` |

The middle two rows are literally shared code. Only `weapon_opts` is new, and it
is the analogue of `look.rs`'s `palette_opts`: the `WEAPON_MATERIALS` row as the
facade's camelCase `opts` bag, including the nested `bake` override (`seed`,
`relief`, `size`) that gives each weapon key its own texture set.

A **bucket name is a material key** — `Assembly::add`'s bucket strings are
exactly the strings the builders pass (`"alu"`, `"steel"`, `"glass"`,
`"cavity"`, …) — so binding is a lookup, not a translation table. The five keys
`WeaponMaterials.get` answers before consulting the table (`cavity`,
`optic_tube`, `glass`, `lens_ring`, `lens_vig`) are resolved too, so the optic
stack stops falling through to grey. An unknown bucket goes through the source's
own `_fallback(key)`.

### 2. `weapons/hands.rs` → drawn

**Correction to the manifest:** `weapons/hands.rs` is *not* unreferenced.
`crate::weapons::viewmodel` constructs both `Arm`s in `Viewmodel::new`
(`viewmodel.rs:608-627`) and `Viewmodel::solve_hands` runs the two-bone IK every
frame inside `late_update` (`viewmodel.rs:1061`). The arms have been fully posed
and solved this whole time. The only missing step was the last one: nothing ever
turned `Arm::meshes` into engine geometry.

`HandGeometry::from_arms` takes both arms apart once at build time, grouping
meshes by **(rig frame, surface)** and merging each group — a `THREE.Mesh` node
in this rig is always at identity relative to its parent (`Arm::add_mesh_node`
writes no transform), so the parent is the frame that actually moves and two
meshes sharing a parent and a surface collapse into one draw. `drive_hands`
then writes one `Transform` per part per frame off
`Arm::update_world_matrix` — the ported `Object3D.updateMatrixWorld` walk, not a
second one.

The four glove surfaces map straight onto four weapon material keys
(`glove`, `glove_pad`, `glove_seam`, `sleeve`), which is why
`WEAPON_MATERIALS` carries exactly four cloth entries.

**The chirality mirror is baked, not transformed.** `handInner.scale.x` is `-1`
on the right arm and it is the *only* non-unit scale in the whole arena. A
`Transform` can carry a `-1` scale, but the renderer transforms normals by the
world matrix rather than by its normal matrix, so a mirrored node would light
from the inside. A pose only ever writes rotations, so the mirror is invariant —
`HandGeometry` reflects that geometry once (positions and normals through
`diag(-1,1,1)`, then the winding reversed) and every per-frame transform stays a
pure rigid motion. Note this is deliberately **not** `Geo::flip_winding`, which
reverses the winding *and negates every normal* — that is the "turn the surface
inside out" operation and would undo the reflection's own normal transform.

---

## Findings

### F1 — the manifest's duplicates table is stale for `weapons`

> | `weapons::system::WeaponCore` | `scene::wiring::weapons` | open |

`scene::wiring::weapons::WeaponsRig` is **not** a thinner copy of `WeaponCore`.
It is a five-field host adapter that *owns* one (`WeaponsRig { core: WeaponCore }`,
`weapons.rs:337`) and exposes it (`core()` / `core_mut()`). Its whole body is
ordering: `fixed_step` → `core.fixed_update`, `frame` → `core.update` +
`core.late_update` + `core.sync_anchor`, in the order the source's engine runs
the three phases that `WeaponSystem::phases()` deliberately returns `&[]` for.
There is no duplicated logic to delete. **No `WeaponCore` merge was needed and
none was done.** The row should be marked *fixed — adapter, not duplicate*.

### F2 — a skinned draw never binds a surface program (engine)

The economical way to draw an arm is one skinned mesh per (arm, surface) with the
node matrices as the joint palette: 8 draws instead of ~80. It was rejected
because of a real backend limit, and this is the finding that matters most out of
this slice:

`axiom-gpu-backend`'s skinned pass sets **one** pipeline for every skinned draw in
the frame — `scene_renderer.rs:2145-2163` binds `skinning.pipeline` once outside
the loop, where the rigid pass at `2100-2137` selects a pipeline per batch from
`programs`. So a skinned draw gets the material bind group (albedo and the four
maps) but **no surface program at all**: `Material::with_surface` is inert on
skinned geometry.

`GpuBackendApi::skinned_surface_degradations` (`surfaces.rs:192`) reports only
*displacement* as dropped on the skinned path, and its doc says "everything else
about a surface lowers identically on both paths." Against the draw loop above,
that is **not true today** — every channel of every surface is dropped, silently.
Either the doc/report is wrong or the pipeline selection is missing;
`SurfaceProgramCatalog::prepare_for(..., GeometryPath::Skinned)` exists and is
called from nowhere but that query and a test, which points at the latter.

Consequence for the wave: **slice 3 (the soldiers) will get flat
albedo-textured Lambert on its skinned bodies, not `ai/textures.rs`'s camo
surface.** Worth telling that slice before it assumes otherwise.

Not fixed here: it is in `modules/`, which this port is not permitted to touch.

### F3 — the vertex-mask lane is hard-zero (engine)

Every weapon material sets `vertexMasks: true`, and the edge wear it selects is
driven by a per-vertex curvature mask (`crate::materials::masks`, baked by
`Arm::bake_surface_masks` / `bake_contact_ao` — both ported, both now
unreachable for rendering). Three things close that path:

- `MeshData` has no colour stream (positions / normals / UVs / joints / weights).
- `RunningApp`'s `interleave_vertices` writes a constant opaque white into the
  vertex stream's colour lane (`resources.rs:131`).
- `material_shader::compose` passes `vec3<f32>(0.0, 0.0, 0.0)` for `vColor` with
  a comment explaining that the lane it *does* have is a tint, not a mask
  (`compose.rs:203-207`).

So the mask reads **zero**: no wear, rather than wear everywhere. The flag is
inert, not wrong, and the gun will read one step cleaner than the source's — the
chamfer highlights are missing, not blown out. Fixing it needs a mask lane on
`MeshData` and a real `vColor` in `compose`, both engine-side.

### F4 — a tint above 1 cannot cross the parameter block

`axiom_surface::MaterialParams::tint` is an sRGB `u32`, decoded with
`hex_to_linear` in `pack()`. The weapon table's tint is a linear
`THREE.Color(r, g, b)` triple, and for the five `metalness: 1` entries it is an
**F0**, not an albedo — `brass` is `(2.3, 1.58, 0.74)`. `linear_to_hex` clamps to
1 and quantises to 8 bits, so brass, copper and `steel_bright` lose their
above-one F0.

This also means the tint cannot ride the `opts` bag at all: `apply_to_params`
reads `"tint"` through `set_hex`, because every *palette* tint is a hex literal.
`library_look` therefore writes it onto the resolved `MaterialParams` directly,
after `engine_params`. That is a deliberate divergence from the source's data
path and is commented at the site.

### F5 — `MeshBasicMaterial` and additive blending have no counterpart

Two of the five custom keys (`lens_ring`, and the reticle materials the optic
would use) are unlit additive overlays. There is no `Material::unlit`; an unlit
material's only route is a surface with `LightingModel::Unlit`, which these keys
have no surface for. They are approximated as a **black albedo carrying the
colour as emissive**, which is the closest the fixed material path gets (a
non-black albedo under an emissive renders as a lit white card). The additive
blend mode is dropped; the material blends on its opacity instead.

### F6 — the hands cost ~80 draws per frame, and that is the honest number

An arm is ~45 authored meshes on ~18 animated frames. Merging per (frame,
surface) takes both arms to roughly eighty parts — the fingers dominate, and they
cannot merge further because each of the twelve finger joints genuinely rotates
(the index finger every frame, off `Arm::set_trigger`). **Nothing is dropped**:
`HandGeometry::draw_count()` reports the exact figure and every part is drawn.
The alternative that would collapse it to 8 is F2, and F2 costs the materials.

### F7 — boot cost

`WeaponLook::new` runs up to fifteen albedo bakes at `RUNTIME_BAKE_SIZE` (64²),
on top of the street's nineteen. Albedo-only at 64² is roughly 55 ms per surface
by the scaling in `RUNTIME_BAKE_SIZE`'s own measured table, so about **0.8 s** of
extra page-load work. It shares no cache with the street's `MaterialSystem` (that
one is owned by `Game` and this is constructed for the `App::install` closure,
which cannot borrow it). Merging the two facades at `Game` and handing this a
`&mut MaterialSystem` is the fix if that second becomes a problem; it is written
up on `WeaponLook`'s doc comment.

---

## What I could not do

- **Verify any of it renders.** No build, no gate, no browser — the wave forbids
  it. Everything above is read against the code, not against a frame.
- **Fix F2 or F3.** Both are in `modules/`. Reported at the boundary rather than
  approximated in the app tier.
- **Give the hands their curvature/contact masks.** `Arm::bake_surface_masks` and
  `Arm::bake_contact_ao` are ported and still have no consumer, and cannot have
  one until F3 is closed. They remain the only part of `hands.rs` that is
  genuinely unreachable.
- **Drive the moving parts.** `install_rifle` still merges the rifle **per
  material**, not per part, so the bolt/mag/trigger animation in
  `Viewmodel::PartsState` still has no per-part nodes to move. Unchanged by this
  slice, and already stated in `drive_viewmodel`'s own doc comment.
- **Use the reticle.** `Viewmodel::update_reticle` solves a collimated dot every
  frame and nothing draws it. It is one more camera-facing quad and it belongs
  with slice 2's pool, not here.

# Slice 3 — the soldiers

Wiring wave, `docs/work-manifests/shmup-port/13-wiring-wave.md`. Nothing was
built, checked, tested or committed; no shared file was touched.

## What was wired

`apps/shmup/src/scene/wiring/soldier_draw.rs` (new) turns the already-ported
soldier into pixels on the engine's **existing** skinning path:

| ported thing | how it now reaches the GPU |
|---|---|
| `ai::soldier::build_soldier` -> `ai::geo::CharacterGeometry` | one `MeshData::new_skinned` per material group, `add_mesh_data` at install |
| `CharacterGeometry.skin_index` / `.skin_weight` | the mesh's joint/weight streams, 1:1 with the compacted vertices |
| `ai::soldier::resolve_materials` -> `MaterialRequest` | one `Material` per group: tint as base colour, roughness, metalness |
| `ai::textures::SoldierMaterials` (the camo bake) | `add_texture_data` x3 per set — albedo + normal + ORM — cached by set name |
| `ai::animator::Animator`'s posed `Skeleton` | `joint_palette()` -> `submit_skinned_draw(.., Transform::IDENTITY, &palette)` |
| `AiCore::update_relevance`'s `lod_irrelevant` | the cull; nothing invents a second frustum test |

The manifest's key fact held: **skinning exists and needed no engine change.**
`submit_skinned_draw` (`modules/axiom/src/app/authoring.rs:88`) plus
`MeshData::new_skinned` plus the `vs_skinned` stage and its joint-palette texture
were all already there.

### The one thing the animator did not produce, and where it now lives

The manifest asked me to state the shape mismatch if there was one. There was a
small one, and it was fixable inside `ai/` rather than being a boundary:

`ai/animator.rs` ports `THREE.Object3D`'s transform graph as `Skeleton` — every
bone's `matrix_world` is current after `Animator::update`. What it did **not**
port is `THREE.Skeleton` itself: `boneInverses` and the `update()` that pairs
them with the bone matrices. Its module header says so outright, and gives the
reason — *"pure skinning state, read only by the unported `SkinnedMesh`"*. That
reason expired the moment `submit_skinned_draw` turned out to be the
`SkinnedMesh`. So the missing half was added where it belongs, next to the bone
hierarchy it pairs with:

* `Mat4::invert` — delegates to the already-ported
  `weapons::rig_math::M4::invert` (identical column-major `[f64; 16]`; a second
  transcription of one THREE function could only drift).
* `Skeleton::bind_inverses(&Rig)` — `THREE.Skeleton.calculateInverses()`.
* `Skeleton::node_of_bone` / `Skeleton::bone_matrix_world` — the bone-to-node
  off-by-one was private, and a caller that guessed it would be silently wrong.
* `Animator::joint_palette(&inverses)` — `THREE.Skeleton.update()`:
  `bone.matrixWorld * boneInverses[i]`, in rig bone order, which is the order
  `skin_index` addresses.

**The inverses are taken with the actor group at the identity**, so a palette
entry already carries the actor's world position, yaw and uniform scale, and the
draw's own transform is `Transform::IDENTITY`. `CharacterGeometry.position` is
authored in exactly that space (`rig.bind_pos`: feet on `y = 0`, facing `+Z`), so
the two pair correctly. THREE reaches the same result by cancelling the group
with the `SkinnedMesh`'s `bindMatrix`/`bindMatrixInverse`; taking the inverse at
the identity makes both of those the identity and deletes the pair. Passing the
actor pose to `submit_skinned_draw` *as well* would apply it twice.

Three unit tests pin this: a bind-pose palette is the identity in every slot;
moving the actor translates every joint by exactly that; and group compaction
preserves the triangle count with no out-of-range index.

### Corrected: a stale claim that had hardened into a design

`scene/wiring/ai.rs`'s `ActorPose` doc asserted *"Axiom has no skinning"* and
concluded the port would need CPU linear-blend skinning with a per-frame mesh
re-upload — "precisely the per-frame geometry churn the engine already learned to
avoid" — and therefore proposed a bind-pose **statue** that slides around the
level. That was wrong when written and it was about to cost the port a whole
subsystem. Both that block and the module's "what is not wired" bullet now point
at `soldier_draw` and say plainly that the claim was never re-checked.

## Cost, and where it stops drawing

`submit_skinned_draw` takes one material per draw, and `CharacterGeometry` is one
vertex buffer partitioned into the nine `MATERIAL_SLOTS` groups. So:

* **9 draws per soldier**, each with its own compacted vertex range and its own
  copy of the same 25-matrix palette.
* `AiCore::populate(2, 3)` garrisons **6** soldiers: **54 skinned draws, 1,350
  palette matrices** per frame.
* The backend's `PALETTE_CAP` is 4,096 (`scene_renderer.rs:772`). **A crowd past
  it stops drawing rather than misdrawing** — the pack loop `break`s. That is a
  ceiling of **27** fully-kitted soldiers on screen. Not a silent per-draw drop:
  it truncates the tail of the frame's skinned list.
* The skinned *instance* buffer is sized once at bind from `max_instances`
  (`scene_renderer.rs:1029`), which this app passes as `renderable_count()`.
  `SoldierDraw::max_draws_per_frame()` exists so the caller can add its headroom;
  the report tells the orchestrator to.

Merging the nine into one draw is **not** a wiring change and was not attempted:
each group samples a different baked set at a different tile scale, so one
material means one atlas and a UV remap this port has no source for.

## What the engine's skinned path drops

Each of these is an engine boundary, reported rather than approximated:

1. **Per-vertex colour — the biggest one.** `CharacterGeometry.color` is the
   baked capsule AO, crevice grime and edge wear: the dark under the plate
   carrier and helmet brim, the rub-through on knees and elbows. It is the thing
   `ai/geo.rs`'s own comment says "stops a procedural character reading as
   plastic". `MeshData` has **no colour stream**, and
   `interleave_skinned_vertices` (`modules/axiom/src/app/resources.rs:139`) writes
   a constant opaque white into the colour lane of every vertex — the rigid path
   does the same. Nothing in the app tier can supply it; the fix is a colour
   stream on `MeshData`, which is engine work.
2. **Emissive and specular.** `vs_skinned` writes both as literal zero
   (`scene_wgsl.rs:517-528`): its pipeline binds all 16 vertex attributes the
   WebGL2 downlevel target guarantees, so the skinned instance payload has no
   lane left. A skinned material renders fully matte. The shader says so and the
   Canvas 2D arm agrees, so the two backends are at least consistent.
3. **The detail tile.** `MaterialRequest`'s `DetailSpec` (the 1.5 mm weave and
   its own UV scale) feeds a shader layer that only runs under a runtime surface
   program, and `vs_skinned` refuses a *displacing* surface outright. Not bound.
4. **`normal_scale` and `ao`.** `MaterialSpec` carries both; `Material` has no
   normal-map strength and no AO strength. Dropped.
5. **`env_map_intensity`** on the goggle glass. No equivalent; the lens is a dark
   opaque dielectric here.

What *does* get through, and matters: the albedo (uploaded `Rgba8UnormSrgb`,
which is exactly the `TextureData::srgb` split the bake records), the
tangent-space normal map and the `(occlusion, roughness, metalness, height)`
pack, all three through `Material`'s texture slots and all three mip-chained and
`AddressMode::Repeat`-sampled — which is what the tile-unit UVs the builder emits
need. Anisotropic sampling is opted in, matching `TextureData::anisotropy`.

## The finding that will bite the integration pass

**`WindowingApi::run_web_multi_skinned` has zero callers in this repository.**
`ax q "run_web_multi_skinned"` returns exactly its own definition
(`modules/axiom-windowing/src/windowing_api/web.rs:219`), and nothing else in the
tree — no app, no test. Its doc comment describes "the soccer arm" writing the
shared cell each frame; that arm is gone.

So: the skinning pipeline **is** exercised, but only through the headless /
off-screen path (`tools/axiom-shot/src/capture.rs:64` hands `skinned_draws` to
`present_frame_result`). The **live browser** entry point is dead code that is
about to be woken up by this slice. `App::run` and `run_web_multi` — which
`shmup_start` currently replicates — upload no skinned meshes and read no skinned
draws at all, which is why switching that one call is a required part of this
slice landing.

Everything under `run_web_multi_skinned` is shared with the arms that do run
(`drive_web_multi`, `present_packet_skinned`, the GPU `Skinning` block), so the
risk is concentrated in the entry point itself, not the pass.

## Not done, and why

* **The contact shadow under each actor.** `ai/grounding.rs` and
  `AiCore::shadow_placements` are ported and compute the placements; drawing one
  needs a ground decal, and the engine has no decal or billboard primitive. It is
  the same pooled camera-facing quad the particles, tracers and shells need, and
  it belongs with them (slice 2), not duplicated here.
* **The carried weapon.** `ai/weapon.rs` builds a rifle per soldier and
  `SoldierBuild.weapon` holds it. It is not part of `CharacterGeometry` — it is
  separate geometry parented to `HandR` — so drawing it is a second, smaller
  version of this same wiring. Left out deliberately rather than half-done: it
  needs its own material resolution and its own bone-follow transform, and
  guessing either would be inventing.
* **Merging the nine draws.** See above — needs an atlas, not wiring.

## Repo hygiene noticed in passing

`fn ratio(f64) -> Ratio` now exists in four places in this app
(`scene/level.rs:409`, `viewer.rs:87`, `scene/wiring/look.rs:500`, and this
file), all private, and they do **not** agree: `look.rs` clamps to `[0, 1]` and
the others do not. That difference is load-bearing here — the variant tints are
deliberately over-unity (`gear_tint` is `[1.08, 0.98, 0.80]`) and `Ratio`
explicitly permits finite magnitudes above 1.0 — so this file documents which one
it is and why rather than reaching for a neighbour's. A single shared helper
would have to be the *unclamped* one, with clamping at the two call sites that
want it; worth doing, but it is not this slice.

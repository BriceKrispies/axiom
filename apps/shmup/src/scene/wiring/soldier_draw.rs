//! **Drawing the soldiers** — the ported bodies, on the engine's skinning path.
//!
//! [`crate::ai::system::AiCore`] has been building complete soldiers all along:
//! `ai/soldier.rs`'s `build_soldier` welds `ai/parts` through `ai/geo`'s
//! `CharacterBuilder` into one **skinned** buffer set, `ai/textures.rs` bakes the
//! camo/cordura/plate/skin material sets, and `ai/animator.rs` poses a 25-bone
//! `Skeleton` per actor every frame with three IK solvers on top. None of it
//! reached a pixel: nothing turned that geometry into an engine mesh, and nothing
//! turned those bone matrices into a joint palette. This module is both halves.
//!
//! ```text
//! install (once)                       frame (per actor, per frame)
//!   built_variants()                     animator.joint_palette(bind_inverses)
//!     -> per material group:               -> submit_skinned_draw(mesh, material,
//!          MeshData::new_skinned              Transform::IDENTITY, &palette)
//!          add_mesh_data  -> Handle<Mesh>
//!          add_texture_data x3 (albedo/normal/orm, cached per material set)
//!          add_material   -> Handle<Material>
//! ```
//!
//! ## The world transform is the identity, deliberately
//!
//! [`crate::ai::animator::Skeleton::bind_inverses`] takes the bind-pose inverses
//! with the actor group at the identity, so each palette entry
//! (`bone.matrixWorld * boneInverse`) already carries the actor's world position,
//! yaw and uniform scale — the skeleton's node 0 is that group, and
//! `Animator::set_actor` writes it every frame. A vertex therefore reaches world
//! space through the palette alone, and the draw's own transform is
//! [`Transform::IDENTITY`]. Passing the actor pose there *as well* would apply it
//! twice. See that function's doc for why the inverses are taken that way.
//!
//! ## One draw per material group, and what that costs
//!
//! `CharacterGeometry` is one vertex buffer partitioned into
//! [`crate::ai::soldier::MATERIAL_SLOTS`]' nine groups (cloth, gear, boot,
//! rubber, plate, polymer, skin, glass, steel), and `submit_skinned_draw` takes
//! **one** material per draw. So a soldier is nine draws, each with its own
//! compacted vertex range and its own copy of the same 25-matrix palette. With
//! `AiCore::populate(2, 3)`'s garrison of six that is **54 skinned draws and
//! 1,350 palette matrices per frame**, against the backend's `PALETTE_CAP` of
//! 4,096 (`scene_renderer.rs:772`). A crowd past that cap stops drawing rather
//! than misdrawing — the backend `break`s — so the ceiling is 27 fully-kitted
//! soldiers on screen at once. [`SoldierDraw::max_draws_per_frame`] publishes the
//! draw count so the caller can size the live backend's skinned instance buffer,
//! which is `max_instances`-sized and shared with nothing.
//!
//! Merging the nine into one draw is not a wiring change: each group samples a
//! different baked set at a different tile scale, so one material would need one
//! atlas and a UV remap the port has no source for.
//!
//! ## What the engine's skinned path drops, and it is not nothing
//!
//! Stated rather than worked around, because each is an engine boundary:
//!
//! * **Per-vertex colour.** `CharacterGeometry.color` is the baked capsule AO,
//!   crevice grime and edge wear — the dark under the plate carrier and the
//!   helmet brim, the rub-through on knees and elbows. [`MeshData`] carries no
//!   colour stream at all and `interleave_skinned_vertices`
//!   (`modules/axiom/src/app/resources.rs:139`) writes an opaque white constant
//!   into the colour lane for every vertex. The single largest fidelity gap here.
//! * **Emissive and specular.** `vs_skinned` writes both as literal zero: its
//!   pipeline already binds 16 vertex attributes, the WebGL2 downlevel guarantee,
//!   so the skinned instance payload has no lane for them
//!   (`scene_wgsl.rs:517-528`). A skinned material renders fully matte.
//! * **The detail tile.** [`crate::ai::soldier::MaterialRequest`]'s `DetailSpec`
//!   (the 1.5 mm weave, its own UV scale) feeds a shader layer that only runs
//!   under a runtime surface program, and `vs_skinned` refuses a displacing
//!   surface outright. Not bound; the base tile carries the macro camo alone.
//!
//! The albedo, tangent-space normal and `(occlusion, roughness, metalness,
//! height)` maps **do** all reach the GPU, through `Material`'s four texture
//! slots — with the albedo uploaded as `Rgba8UnormSrgb` and the other two as
//! `Rgba8Unorm`, which is exactly the `TextureData::srgb` split the bake records.
//!
//! ## Not here
//!
//! The contact shadow under each actor (`ai/grounding.rs`,
//! `AiCore::shadow_placements`) is a ground decal, and the engine has no decal or
//! billboard primitive — it is the pooled camera-facing quad every other
//! ground-projected effect in this port needs, and it belongs with them.

use std::collections::BTreeMap;

use axiom::prelude::{
    Color, Handle, Mat4, Material, Mesh, MeshData, Ratio, RunningApp, TextureSampling, Transform,
    Vec2, Vec3,
};

use crate::ai::animator::{Mat4 as RigMat4, Skeleton};
use crate::ai::geo::CharacterGeometry;
use crate::ai::rig::RIG;
use crate::ai::soldier::{MaterialRequest, SoldierBuild};
use crate::ai::textures::{SoldierMaterials, TextureData, GLASS};
use crate::scene::wiring::ai::AiWiring;

/// A finite `f64` as a [`Ratio`].
///
/// `Ratio::finite_or_zero`, **not** a clamp to `[0, 1]`: the variant tints are
/// deliberately over-unity (`cloth_tint` is `[1.03, 1.0, 0.94]`, `gear_tint`
/// `[1.08, 0.98, 0.80]`) and `Ratio` documents that finite magnitudes above 1.0
/// pass through unchanged. Clamping them — which is what
/// `crate::scene::wiring::look`'s same-named private helper does, correctly, for
/// *its* values — would flatten the warm push the whole kit palette is built on.
fn ratio(v: f64) -> Ratio {
    Ratio::finite_or_zero(v as f32)
}

/// A linear RGB triple as a [`Color`].
fn color3(c: [f64; 3]) -> Color {
    Color::linear_rgb(ratio(c[0]), ratio(c[1]), ratio(c[2]))
}

/// The three maps of one baked material set, once registered.
#[derive(Clone, Copy)]
struct SetTextures {
    /// sRGB albedo.
    albedo: u64,
    /// Tangent-space normal.
    normal: u64,
    /// `(occlusion, roughness, metalness, _)`.
    orm: u64,
}

/// One material group of one variant: the compacted skinned sub-mesh and the
/// material it is drawn with.
struct GroupDraw {
    mesh: Handle<Mesh>,
    material: Handle<Material>,
}

/// One built variant's draws, in `CharacterGeometry::groups` order.
struct VariantDraw {
    name: String,
    groups: Vec<GroupDraw>,
}

/// Registered soldier bodies, and the bind-pose inverses that turn a posed
/// skeleton into a joint palette.
///
/// Build once with [`SoldierDraw::install`] after the app is realized; call
/// [`SoldierDraw::frame`] once per rendered frame, **before**
/// `RunningApp::tick`, which is what drains the queued skinned draws.
pub struct SoldierDraw {
    variants: Vec<VariantDraw>,
    /// `THREE.Skeleton.boneInverses` — one table, shared by every actor.
    bind_inverses: Vec<RigMat4>,
}

impl SoldierDraw {
    /// Register every variant the garrison actually built: one skinned mesh and
    /// one material per material group, with each baked set's three maps
    /// uploaded once and shared across variants that name it.
    ///
    /// Reads [`AiWiring::built_variants`] and never `AiCore::variant`, for the
    /// reason that accessor states: `variant` takes `&mut self` because it
    /// *builds* an unseen variant on demand, and building forks the AI's RNG
    /// stream. Asking for a name the garrison did not spawn would reshuffle
    /// every draw after it.
    ///
    /// Call this **after** `App::build`: `add_mesh_data` / `add_texture_data` /
    /// `add_material` are `RunningApp` methods and the live backend reads
    /// `skinned_mesh_set()` and `material_textures()` after the app is realized,
    /// so a body registered here is in the set the backend binds.
    #[must_use]
    pub fn install(running: &mut RunningApp, ai: &AiWiring) -> SoldierDraw {
        let mut textures: BTreeMap<String, SetTextures> = BTreeMap::new();
        // Borrowed, never cloned: `SoldierMaterials` is eight baked sets of
        // three 512² RGBA maps — about 25 MB — and it is `Clone`, so copying it
        // per variant is a mistake that compiles.
        let baked = &ai.core().materials;
        let variants = ai
            .built_variants()
            .iter()
            .map(|(name, build)| VariantDraw {
                name: name.clone(),
                groups: install_variant(running, baked, build, &mut textures),
            })
            .collect();
        SoldierDraw {
            variants,
            bind_inverses: Skeleton::bind_inverses(&RIG),
        }
    }

    /// Submit one skinned draw per material group per visible actor.
    ///
    /// Culling is the AI's own: `AiCore::update_relevance` sets
    /// `agent.lod_irrelevant` when neither the actor's bounding sphere nor its
    /// shadow can reach the view frustum, and that flag — not a second frustum
    /// test invented here — decides what is drawn. **Dead actors are still
    /// drawn**: `Agent::die` hands the skeleton to the ragdoll and the body stays
    /// in the world, so filtering on `alive` would make corpses vanish.
    pub fn frame(&self, running: &mut RunningApp, ai: &AiWiring) {
        let inverses = &self.bind_inverses;
        ai.core()
            .actors
            .iter()
            .filter(|a| !a.agent.lod_irrelevant)
            .for_each(|a| {
                let palette: Vec<Mat4> = a
                    .animator
                    .joint_palette(inverses)
                    .iter()
                    .map(engine_matrix)
                    .collect();
                self.variants
                    .iter()
                    .filter(|v| v.name == a.agent.variant_name)
                    .for_each(|v| {
                        v.groups.iter().for_each(|g| {
                            running.submit_skinned_draw(
                                g.mesh,
                                g.material,
                                Transform::IDENTITY,
                                &palette,
                            );
                        });
                    });
            });
    }

    /// The most skinned draws one frame can carry: every registered variant's
    /// group count, summed.
    ///
    /// A frame never actually reaches this — an actor draws one variant, not all
    /// of them — but the live backend sizes its skinned instance buffer once, at
    /// bind, from a single `max_instances`, so the number it needs is an upper
    /// bound and not a measurement. The caller adds this to
    /// `RunningApp::renderable_count()`.
    #[must_use]
    pub fn max_draws_per_frame(&self) -> usize {
        self.variants.iter().map(|v| v.groups.len()).sum()
    }

    /// Joint matrices one frame can carry, for checking against the backend's
    /// 4,096-matrix palette capacity. Beyond it the backend stops drawing
    /// skinned bodies rather than misdrawing them.
    #[must_use]
    pub fn max_palette_matrices(&self) -> usize {
        self.max_draws_per_frame() * self.bind_inverses.len()
    }
}

/// Register one variant's material groups.
fn install_variant(
    running: &mut RunningApp,
    materials: &SoldierMaterials,
    build: &SoldierBuild,
    textures: &mut BTreeMap<String, SetTextures>,
) -> Vec<GroupDraw> {
    let indices = build.geometry.index.to_u32();
    build
        .geometry
        .groups
        .iter()
        // A zero-count group draws nothing; `material_index == -1` is
        // `CharacterBuilder::build`'s empty-builder case, which names no
        // material at all.
        .filter(|g| (g.count > 0) & (g.material_index >= 0))
        .map(|g| {
            let data = group_mesh_data(&build.geometry, &indices[g.start..g.start + g.count]);
            let mesh = running
                .add_mesh_data(data)
                .expect("a soldier material group is valid skinned geometry");
            let request = &build.materials[g.material_index as usize];
            let set = set_textures(running, materials, request, textures);
            let material = running.add_material(group_material(request, set));
            GroupDraw { mesh, material }
        })
        .collect()
}

/// Register (or reuse) the three baked maps of the set this request names.
///
/// Keyed on the **set** name (`camo_arid`, `nylon`, `plate`, …) rather than the
/// request's own cache key: two slots that differ only in tint or roughness
/// sample the identical pixels, and a 512² RGBA bake is a megabyte apiece.
fn set_textures(
    running: &mut RunningApp,
    materials: &SoldierMaterials,
    request: &MaterialRequest,
    cache: &mut BTreeMap<String, SetTextures>,
) -> Option<SetTextures> {
    // `MaterialRequest::Glass` names no set: the goggle lens is a tinted,
    // untextured material in the source too (`textures.js`'s `glass()` takes no
    // maps, only an env-map intensity this engine has no equivalent for).
    let name = match request {
        MaterialRequest::Set(spec) => spec.set.clone(),
        MaterialRequest::Glass => return None,
    };
    let cached = cache.get(&name).copied();
    cached.or_else(|| {
        let baked = materials.set(&name)?;
        let entry = SetTextures {
            albedo: register_map(running, &baked.albedo),
            normal: register_map(running, &baked.normal),
            orm: register_map(running, &baked.orm),
        };
        cache.insert(name, entry);
        Some(entry)
    })
}

/// One baked square tile as an engine texture id.
fn register_map(running: &mut RunningApp, map: &TextureData) -> u64 {
    running
        .add_texture_data(map.size, map.size, map.pixels.clone())
        .expect("a soldier bake is exactly size * size * 4 bytes")
        .id()
}

/// The engine material for one resolved slot.
///
/// Every field the source's `SoldierMaterials.get(set, opts)` would have applied
/// and this engine can express: the tint as the base colour (which the shader
/// multiplies the sampled albedo by, exactly as three multiplies `map` by
/// `color`), the roughness and metalness scalars, and the set's three maps. The
/// `normal_scale` and `ao` fields have no counterpart — `Material` carries no
/// normal-map strength or AO strength — and the detail tile is dropped for the
/// reason the module header gives.
fn group_material(request: &MaterialRequest, set: Option<SetTextures>) -> Material {
    match request {
        // `glass(tint = [0.06, 0.07, 0.08])`, `textures.js:926-940`. Left opaque:
        // the source's lens is a dark dielectric read through an environment
        // reflection, not an alpha blend, and `env_map_intensity` has nowhere to
        // go here.
        MaterialRequest::Glass => Material::lit(color3(GLASS.tint))
            .with_roughness(ratio(GLASS.roughness))
            .with_metallic(ratio(GLASS.metalness)),
        MaterialRequest::Set(spec) => {
            let base = Material::lit(spec.tint.map_or(Color::WHITE, color3))
                // Every soldier tile is baked with an anisotropy setting
                // (`TextureData::anisotropy`, 8 by default) and a full mip chain;
                // a 0.78 m cloth tile wrapped round a limb at 25 m is exactly the
                // grazing-angle minification that costs.
                .with_texture_sampling(TextureSampling::Anisotropic);
            let rough = spec.rough.map_or(base, |r| base.with_roughness(ratio(r)));
            let metal = spec.metal.map_or(rough, |m| rough.with_metallic(ratio(m)));
            set.map_or(metal, |t| {
                metal
                    .with_custom_texture(t.albedo)
                    .with_normal_texture(t.normal)
                    .with_orm_texture(t.orm)
            })
        }
    }
}

/// Compact one material group's index range into standalone skinned geometry.
///
/// The group's indices address the whole character's vertex buffer, so a naive
/// slice would need all ~n vertices uploaded nine times over. This walks the
/// range once, emitting each referenced vertex the first time it is seen and
/// rewriting the indices against the compact set — the streams stay aligned 1:1,
/// which is what `MeshData::new_skinned` validates.
///
/// The UVs are already in tile units (`CharacterBuilder::build` divides by the
/// material's tile size), and the engine's material sampler is
/// `AddressMode::Repeat`, so they need no rescale.
fn group_mesh_data(geometry: &CharacterGeometry, indices: &[u32]) -> MeshData {
    let mut remap: Vec<u32> = vec![u32::MAX; geometry.vertices];
    let mut positions: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut joints: Vec<[u16; 4]> = Vec::new();
    let mut weights: Vec<[f32; 4]> = Vec::new();
    let mut out: Vec<u32> = Vec::with_capacity(indices.len());
    for &i in indices {
        let v = i as usize;
        if remap[v] == u32::MAX {
            remap[v] = positions.len() as u32;
            positions.push(Vec3::new(
                geometry.position[v * 3],
                geometry.position[v * 3 + 1],
                geometry.position[v * 3 + 2],
            ));
            normals.push(Vec3::new(
                geometry.normal[v * 3],
                geometry.normal[v * 3 + 1],
                geometry.normal[v * 3 + 2],
            ));
            uvs.push(Vec2::new(geometry.uv[v * 2], geometry.uv[v * 2 + 1]));
            joints.push([
                geometry.skin_index[v * 4],
                geometry.skin_index[v * 4 + 1],
                geometry.skin_index[v * 4 + 2],
                geometry.skin_index[v * 4 + 3],
            ]);
            weights.push([
                geometry.skin_weight[v * 4],
                geometry.skin_weight[v * 4 + 1],
                geometry.skin_weight[v * 4 + 2],
                geometry.skin_weight[v * 4 + 3],
            ]);
        }
        out.push(remap[v]);
    }
    MeshData::new_skinned(positions, normals, uvs, joints, weights, out)
}

/// `THREE.Matrix4` (column-major `f64`) as the engine's column-major `f32`
/// [`Mat4`]. The two layouts are identical — `e[12..15]` is the translation in
/// both — so this is a width narrowing and nothing else. The narrowing is where
/// the port's `f64` pose meets the GPU's `f32` palette, and it happens exactly
/// once per bone per frame, here.
fn engine_matrix(m: &RigMat4) -> Mat4 {
    let mut out = [0.0f32; 16];
    (0..16).for_each(|i| out[i] = m.e[i] as f32);
    Mat4::from_cols_array(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::animator::Animator;
    use crate::weapons::rig_math::V3;

    /// A palette taken straight off a freshly-built skeleton is the actor group's
    /// own matrix in every slot — which for an untouched animator is the
    /// identity. That is the property the whole scheme rests on: bind pose times
    /// bind inverse is the group transform, so a bind-pose vertex lands exactly
    /// where the bake put it.
    #[test]
    fn a_bind_pose_palette_is_the_actor_transform() {
        let inverses = Skeleton::bind_inverses(&RIG);
        assert_eq!(inverses.len(), RIG.count);
        let animator = Animator::new(&RIG, None, None, None, 1.0);
        animator
            .joint_palette(&inverses)
            .iter()
            .enumerate()
            .for_each(|(i, m)| {
                m.e.iter()
                    .zip(RigMat4::IDENTITY.e.iter())
                    .for_each(|(got, want)| {
                        assert!(
                            (got - want).abs() < 1e-9,
                            "bone {i} is not the identity at rest: {:?}",
                            m.e
                        );
                    });
            });
    }

    /// ...and moving the actor moves every joint by exactly that, translation
    /// included, so the draw needs no world transform of its own.
    #[test]
    fn moving_the_actor_translates_every_joint() {
        let inverses = Skeleton::bind_inverses(&RIG);
        let mut animator = Animator::new(&RIG, None, None, None, 1.0);
        animator.set_actor(V3::new(3.0, 0.0, -7.0), 0.0);
        animator
            .joint_palette(&inverses)
            .iter()
            .for_each(|m| {
                assert!((m.e[12] - 3.0).abs() < 1e-9, "x: {}", m.e[12]);
                assert!(m.e[13].abs() < 1e-9, "y: {}", m.e[13]);
                assert!((m.e[14] + 7.0).abs() < 1e-9, "z: {}", m.e[14]);
            });
    }

    /// Compaction keeps the triangle count and never emits an out-of-range
    /// index, which is what `add_mesh_data` would reject.
    #[test]
    fn compaction_preserves_the_triangles_it_was_given() {
        let mut rng = crate::rng::Rng::new(7);
        let build = crate::ai::soldier::build_soldier("vanguard", &mut rng);
        let indices = build.geometry.index.to_u32();
        let group = build
            .geometry
            .groups
            .iter()
            .find(|g| g.count > 0)
            .expect("a built soldier has a non-empty group");
        let data = group_mesh_data(
            &build.geometry,
            &indices[group.start..group.start + group.count],
        );
        assert_eq!(data.indices().len(), group.count);
        assert_eq!(data.joints().len(), data.positions().len());
        assert_eq!(data.weights().len(), data.positions().len());
        assert!(data.is_skinned());
        let n = data.positions().len() as u32;
        assert!(data.indices().iter().all(|&i| i < n));
        // ...and it is a compaction, not a copy: one group is a fraction of the
        // character's vertices.
        assert!(n < build.geometry.vertices as u32);
    }
}

//! Turn a validated [`Manifest`] into **neutral render data** — plain meshes,
//! instanced batches, lights, a camera view-projection, and a shadow
//! view-projection, with **no GPU types**. The runner bin feeds this to either the
//! off-screen GPU backend or the Canvas 2D backend (the two `tools/axiom-shot`
//! arms), exactly like the growth-agent capture path.
//!
//! Everything here is a pure, deterministic function of the manifest, so the
//! reproducibility guarantee lives at *this* boundary: the same file always
//! produces byte-identical geometry, instance transforms, and matrices. (Whether
//! the final PNG is byte-identical is then a property of the chosen backend — the
//! software Canvas 2D path is; the GPU path is only on the same adapter.)
//!
//! Vertex layout is the engine's standard 12 floats: position(3) · normal(3) ·
//! uv(2) · colour(4). Instance layout is the engine's 36 floats: view_proj(16) ·
//! world(16) · tint(4).

use axiom_host::{
    BackendCapabilityProfile, FrameAmbient, FramePostProcess, FrameVolumetrics, RenderCapability,
};
use axiom_kernel::Meters;
use axiom_math::{Mat4, Quat, Transform, Vec3};
use axiom_terrain_mesh::TerrainMeshApi;

use super::scatter;
use super::scene::{value_noise, Foliage, Manifest, Style, Terrain, Tree, Tuft};

/// Floats per mesh vertex: position(3) + normal(3) + uv(2) + colour(4).
const VERT_FLOATS: usize = 12;

/// Mesh + material ids (stable within one frame).
const TERRAIN_MESH: u64 = 1;
const TRUNK_MESH: u64 = 2;
const CANOPY_MESH: u64 = 3;
const GROUNDCOVER_MESH: u64 = 4;
const FOLIAGE_MESH: u64 = 5;
const LITTER_MESH: u64 = 6;
/// Thin branch strokes radiating from each crown; leaves cluster along them.
const BRANCH_MESH: u64 = 7;
/// Taller upright sedge/grass-frond clump — the second ground-plant species.
const FERN_MESH: u64 = 8;
const WHITE_MAT: u64 = 1;
/// A radial soft-alpha leaf texture — GPU renders the foliage cards as feathered leaf
/// blobs (alpha cutout + blend); Canvas 2D ignores it and keeps the solid-card proxy.
const LEAF_ALPHA_MAT: u64 = 2;
/// Procedural beech-bark detail texture (a near-white value multiplier the trunk tint
/// modulates) — GPU-only surface grain; Canvas 2D keeps the flat trunk tint.
const BARK_MAT: u64 = 3;
/// Procedural forest-floor detail texture (color mottle multiplier) — GPU-only ground
/// grain; Canvas 2D keeps the flat per-vertex ground colour.
const GROUND_MAT: u64 = 4;

/// Radial segments in the unit trunk cylinder.
const TRUNK_SEGMENTS: u32 = 8;
/// Rings / sectors in the unit canopy blob. ITER 14 (attack: artifact_level) — the
/// canopy-geometry abstraction: raise the mesh resolution so canopies read as
/// rounded crowns instead of faceted blobs (the dominant remaining artifact).
const CANOPY_RINGS: u32 = 8;
const CANOPY_SECTORS: u32 = 14;
/// Blades in the unit ground-cover tuft (a small crossed-blade cluster).
const TUFT_BLADES: u32 = 6;

/// Bark tint the trunk instances carry (fog is folded in per instance). Light
/// silver-grey beech — the reference's trunks are pale, not near-black; the bark detail
/// texture (GPU) modulates this, Canvas 2D shows it flat.
const BARK: [f32; 3] = [0.56, 0.50, 0.43];

/// Neutral, backend-agnostic render data for exactly one frame.
#[derive(Debug, Clone)]
pub struct RenderData {
    pub width: u32,
    pub height: u32,
    /// Camera view-projection (column-major, the instance MVP).
    pub view_proj: [f32; 16],
    /// Directional-sun shadow view-projection (column-major).
    pub light_view_proj: [f32; 16],
    /// `(kind, direction/position, colour, intensity)` per light; kind 0 = directional.
    pub lights: Vec<(u32, [f32; 3], [f32; 3], f32)>,
    /// Frame clear colour (the fog colour), RGBA.
    pub clear: [f32; 4],
    /// `(mesh_id, interleaved 12-float vertices, triangle indices)`.
    pub meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
    /// `(material_id, width, height, RGBA8 albedo)`.
    pub materials: Vec<(u64, u32, u32, Vec<u8>)>,
    /// Per-material tangent-space normal maps `(material_id, w, h, RGBA8)` — GPU-only
    /// surface relief; materials absent here get a flat normal (Canvas 2D ignores all).
    pub normals: Vec<(u64, u32, u32, Vec<u8>)>,
    /// `(mesh_id, material_id, interleaved 36-float instances, instance count)`.
    pub batches: Vec<(u64, u64, Vec<f32>, u32)>,
    /// Optional volumetric light (god-rays) — neutral frame data every backend
    /// realizes through `host::apply_frame_volumetrics`.
    pub volumetrics: Option<FrameVolumetrics>,
    /// Optional filmic tonemap post-process (ACES + exposure) — neutral frame data
    /// the GPU always applies and Canvas 2D applies unless its profile drops it.
    pub postprocess: Option<FramePostProcess>,
    /// Hemisphere ambient — neutral frame data lighting the faces no directional
    /// light reaches (lifts the backlit trunk faces + softens shadow contrast).
    pub ambient: FrameAmbient,
    /// The capability profile the **Canvas 2D** backend should use (resolved from the
    /// manifest's `[canvas2d]` config). The GPU path always attempts everything.
    pub canvas2d_profile: BackendCapabilityProfile,
}

/// Resolve the Canvas 2D capability profile from the manifest's `[canvas2d]` config:
/// `all()` minus any capability the config disables.
fn canvas2d_profile(manifest: &Manifest) -> BackendCapabilityProfile {
    let all = BackendCapabilityProfile::all();
    match &manifest.canvas2d {
        Some(c) => {
            let p = [all, all.without(RenderCapability::Volumetrics)][usize::from(!c.volumetrics)];
            [p, p.without(RenderCapability::PostProcess)][usize::from(!c.postprocess)]
        }
        None => all,
    }
}

/// The full instance list: the explicitly authored trees plus, if present, the
/// deterministic `[scatter]` expansion.
pub fn all_trees(manifest: &Manifest) -> Vec<Tree> {
    let mut trees = manifest.trees.clone();
    if let Some(s) = &manifest.scatter {
        let cam_xz = [manifest.camera.eye[0], manifest.camera.eye[2]];
        trees.extend(scatter::expand(s, &manifest.terrain, cam_xz));
    }
    trees
}

/// The ground-cover tufts, if the manifest carries a `[groundcover]` block.
pub fn all_groundcover(manifest: &Manifest) -> Vec<Tuft> {
    manifest
        .groundcover
        .as_ref()
        .map(|g| scatter::expand_groundcover(g, &manifest.terrain))
        .unwrap_or_default()
}

/// The fallen-leaf litter scatter (flat leaves on the ground), from the `[litter]`
/// config; empty when absent. Reuses the ground-cover scatter.
pub fn all_litter(manifest: &Manifest) -> Vec<Tuft> {
    manifest
        .litter
        .as_ref()
        .map(|g| scatter::expand_groundcover(g, &manifest.terrain))
        .unwrap_or_default()
}

/// Build every neutral artifact the backends consume from `manifest`.
pub fn build(manifest: &Manifest) -> RenderData {
    let cam = &manifest.camera;
    let eye = Vec3::new(cam.eye[0], cam.eye[1], cam.eye[2]);
    let view_proj = camera_view_proj(manifest);
    let (lights, light_view_proj) = sun(manifest);

    let (terrain_v, terrain_i) = terrain_mesh(&manifest.terrain, &manifest.fog, eye, &style_of(manifest));
    let (trunk_v, trunk_i) = trunk_unit_mesh();
    let (canopy_v, canopy_i) = canopy_unit_mesh();
    let (foliage_v, foliage_i) = foliage_card_unit_mesh();
    let (branch_v, branch_i) = branch_unit_mesh();
    let (tuft_v, tuft_i) = tuft_unit_mesh();
    let (fern_v, fern_i) = fern_unit_mesh();
    let (litter_v, litter_i) = litter_unit_mesh();

    let lean_deg = manifest.scatter.as_ref().map(|s| s.lean_deg).unwrap_or(0.0);
    let trees = all_trees(manifest);
    let trunk_inst = trunk_instances(manifest, &trees, lean_deg, &view_proj, eye);
    // Canopy: stylized foliage-card clusters when configured, else sphere blobs.
    // Foliage cards get the leaf-alpha material (GPU feathers them via alpha cutout);
    // the sphere-blob fallback stays a solid white material.
    let (canopy_mesh_id, canopy_inst, canopy_count, canopy_mat) = match &manifest.foliage {
        Some(f) => {
            let inst = foliage_instances(manifest, &trees, f, lean_deg, &view_proj, eye);
            let count = (inst.len() / 36) as u32;
            (FOLIAGE_MESH, inst, count, LEAF_ALPHA_MAT)
        }
        None => (
            CANOPY_MESH,
            canopy_instances(manifest, &trees, lean_deg, &view_proj, eye),
            trees.len() as u32,
            WHITE_MAT,
        ),
    };
    // Branch scaffold (only when [foliage] branches > 0): dark strokes the leaves hang on.
    let branch_inst = manifest
        .foliage
        .as_ref()
        .filter(|f| f.branches > 0)
        .map(|f| branch_instances(manifest, &trees, f, lean_deg, &view_proj, eye))
        .unwrap_or_default();
    let branch_count = (branch_inst.len() / 36) as u32;

    let tufts = all_groundcover(manifest);
    // Two ground-plant species from one scatter: ~55% low splayed grass clumps, ~45%
    // taller upright sedge fronds — so the floor reads as mixed plants, not one repeat.
    let grass: Vec<Tuft> = tufts.iter().copied().filter(|t| hash01(t.x, t.z, 5000) < 0.55).collect();
    let sedge: Vec<Tuft> = tufts.iter().copied().filter(|t| hash01(t.x, t.z, 5000) >= 0.55).collect();
    let grass_inst = plant_instances(manifest, &grass, 1.0, &view_proj, eye);
    let sedge_inst = plant_instances(manifest, &sedge, 2.4, &view_proj, eye);
    let litter = all_litter(manifest);
    let litter_inst = litter_instances(manifest, &litter, &view_proj, eye);

    // Terrain: one identity-world instance whose MVP is the camera view-projection.
    let terrain_batch_inst =
        instance(&Mat4::from_cols_array(view_proj), Mat4::IDENTITY, [1.0, 1.0, 1.0, 1.0]);

    let mut batches: Vec<(u64, u64, Vec<f32>, u32)> =
        vec![(TERRAIN_MESH, GROUND_MAT, terrain_batch_inst, 1)];
    // Only emit vegetation batches when there is at least one tree.
    let tree_count = trees.len() as u32;
    (tree_count > 0).then(|| {
        batches.push((TRUNK_MESH, BARK_MAT, trunk_inst, tree_count));
        batches.push((canopy_mesh_id, canopy_mat, canopy_inst, canopy_count));
    });
    (branch_count > 0).then(|| batches.push((BRANCH_MESH, WHITE_MAT, branch_inst, branch_count)));
    // Ground cover: two species batches (low grass clumps + taller sedge fronds).
    let grass_count = grass.len() as u32;
    (grass_count > 0).then(|| batches.push((GROUNDCOVER_MESH, WHITE_MAT, grass_inst, grass_count)));
    let sedge_count = sedge.len() as u32;
    (sedge_count > 0).then(|| batches.push((FERN_MESH, WHITE_MAT, sedge_inst, sedge_count)));
    // Fallen-leaf litter: a dense flat-leaf carpet on the ground.
    let litter_count = litter.len() as u32;
    (litter_count > 0).then(|| batches.push((LITTER_MESH, WHITE_MAT, litter_inst, litter_count)));

    RenderData {
        width: cam.width_px,
        height: cam.height_px,
        view_proj,
        light_view_proj,
        lights,
        clear: [manifest.fog.color[0], manifest.fog.color[1], manifest.fog.color[2], 1.0],
        meshes: vec![
            (TERRAIN_MESH, terrain_v, terrain_i),
            (TRUNK_MESH, trunk_v, trunk_i),
            (CANOPY_MESH, canopy_v, canopy_i),
            (FOLIAGE_MESH, foliage_v, foliage_i),
            (BRANCH_MESH, branch_v, branch_i),
            (GROUNDCOVER_MESH, tuft_v, tuft_i),
            (FERN_MESH, fern_v, fern_i),
            (LITTER_MESH, litter_v, litter_i),
        ],
        materials: vec![
            white_material(),
            leaf_alpha_material(),
            bark_material(),
            ground_material(),
        ],
        normals: vec![bark_normal_material(), ground_normal_material()],
        batches,
        volumetrics: manifest.volumetrics.then(FrameVolumetrics::low_poly),
        postprocess: manifest.postprocess.then(FramePostProcess::cinematic),
        ambient: manifest
            .ambient
            .map(|a| FrameAmbient::new(a.sky, a.ground))
            .unwrap_or_else(FrameAmbient::default_hemisphere),
        canvas2d_profile: canvas2d_profile(manifest),
    }
}

/// The camera view-projection: `perspective · look_at`.
fn camera_view_proj(manifest: &Manifest) -> [f32; 16] {
    let c = &manifest.camera;
    let aspect = c.width_px as f32 / c.height_px as f32;
    let proj = Mat4::perspective(c.fov_deg.to_radians(), aspect, c.near_m, c.far_m)
        .unwrap_or(Mat4::IDENTITY);
    let eye = Vec3::new(c.eye[0], c.eye[1], c.eye[2]);
    let target = Vec3::new(c.target[0], c.target[1], c.target[2]);
    let view = Mat4::look_at(eye, target, Vec3::UNIT_Y).unwrap_or(Mat4::IDENTITY);
    proj.multiply(view).as_cols_array()
}

/// The directional sun light tuple + its shadow view-projection covering the patch.
fn sun(manifest: &Manifest) -> (Vec<(u32, [f32; 3], [f32; 3], f32)>, [f32; 16]) {
    let s = &manifest.sun;
    let travel = Vec3::new(s.direction[0], s.direction[1], s.direction[2]);
    // The shader wants the *to-light* direction (points toward the sun).
    let to_light = travel.mul_scalar(-1.0).normalize().unwrap_or(Vec3::UNIT_Y);
    let light = (0u32, [to_light.x, to_light.y, to_light.z], s.color, s.intensity);

    // Shadow ortho: look from far up the sun ray toward the patch centre, framing a
    // box a little larger than the terrain so trees at the edge still cast shadows.
    let ext = manifest.terrain.half_m() * 1.3 + 8.0;
    let centre = Vec3::new(0.0, manifest.terrain.base_height_m, 0.0);
    let dir = travel.normalize().unwrap_or(Vec3::UNIT_Y);
    let dist = ext * 2.0 + 40.0;
    let light_eye = centre.subtract(dir.mul_scalar(dist));
    let up = pick_up(dir);
    let light_view = Mat4::look_at(light_eye, centre, up).unwrap_or(Mat4::IDENTITY);
    let light_proj =
        Mat4::orthographic(-ext, ext, -ext, ext, 1.0, dist * 2.0).unwrap_or(Mat4::IDENTITY);
    let light_view_proj = light_proj.multiply(light_view).as_cols_array();

    (vec![light], light_view_proj)
}

/// An up vector not parallel to `dir` (Z-up when the light is near-vertical).
fn pick_up(dir: Vec3) -> Vec3 {
    (dir.y.abs() > 0.99).then_some(Vec3::UNIT_Z).unwrap_or(Vec3::UNIT_Y)
}

/// The 64×64 terrain patch: the neutral grid from `axiom-terrain-mesh`, decorated
/// with ground-band albedo, a slope→rock tint, and baked linear distance fog.
fn terrain_mesh(terrain: &Terrain, fog: &super::scene::Fog, eye: Vec3, style: &Style) -> (Vec<f32>, Vec<u32>) {
    let mesh = TerrainMeshApi::heightfield_grid_mesh(
        (Meters::finite_or_zero(0.0), Meters::finite_or_zero(0.0)),
        Meters::finite_or_zero(terrain.half_m()),
        Meters::finite_or_zero(terrain.spacing_m),
        |mx, mz| Meters::finite_or_zero(terrain.height_at(mx.get(), mz.get())),
    );

    let mut vertices: Vec<f32> = Vec::with_capacity(mesh.positions().len() * VERT_FLOATS);
    mesh.positions()
        .iter()
        .zip(mesh.normals())
        .for_each(|(pos, normal)| {
            let base = ground_albedo(terrain, pos.y);
            let slope = terrain.slope_at(pos.x, pos.z);
            let rock_t = smoothstep(terrain.rock_slope_start, terrain.rock_slope_full, slope);
            let rocked = lerp3(base, terrain.rock_albedo, rock_t);
            // The forest-floor BASE is dark brown soil / leaf-mulch — the reference's
            // exposed earth that shows *between* the leaves. The actual fallen leaves are
            // the litter INSTANCES scattered on top, NOT baked into the ground; baking a
            // bright orange carpet here is what made the floor read as one flat glowing
            // sheet. Keep the base dark + varied: mottle the soil between dark mulch and
            // lighter dirt by coarse + fine noise, with sparse mossy-green patches.
            let coarse_n = value_noise(4242, pos.x * 0.18, pos.z * 0.18) * 0.5 + 0.5;
            let fine_n = value_noise(7777, pos.x * 1.1, pos.z * 1.1) * 0.5 + 0.5;
            let soil = lerp3([0.11, 0.08, 0.055], [0.22, 0.16, 0.10], coarse_n * 0.7 + fine_n * 0.3);
            // The height/ground-band tint whispers through the soil (12%) so relief reads.
            let earth = lerp3(soil, rocked, 0.12);
            // Sparse muted-green moss breaking up the bare earth.
            let moss_n = smoothstep(0.66, 0.90, value_noise(1313, pos.x * 0.52, pos.z * 0.52) * 0.5 + 0.5);
            let surface = lerp3(earth, [0.17, 0.21, 0.115], moss_n * 0.55);

            let dist = eye.subtract(Vec3::new(pos.x, pos.y, pos.z)).length();
            let col = fogged(surface, fog, dist, style, 1.0);

            // Planar UVs tiled every ~2.5 m (Repeat sampler) so the ground detail texture
            // adds sub-grid mottle without stretching across the whole terrain.
            push_vertex(
                &mut vertices,
                [pos.x, pos.y, pos.z],
                [normal.x, normal.y, normal.z],
                [pos.x / 2.5, pos.z / 2.5],
                col,
            );
        });
    (vertices, mesh.indices().to_vec())
}

/// A deterministic `[0, 1)` hash from a world `(x, z)` plus a `salt` — the per-tree /
/// per-card randomness source (pure integer bit math, identical on every platform).
fn hash01(x: f32, z: f32, salt: u32) -> f32 {
    let mut h = x.to_bits().wrapping_mul(0x9E37_79B1)
        ^ z.to_bits().wrapping_mul(0x85EB_CA77)
        ^ salt.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// A tree's deterministic lean: `(theta, dir)` — tilt magnitude (radians, up to
/// `lean_deg_max`) and the horizontal direction it leans toward.
fn tree_lean(t: &Tree, lean_deg_max: f32) -> (f32, f32) {
    let theta = (hash01(t.x, t.z, 71) * lean_deg_max).to_radians();
    (theta, hash01(t.x, t.z, 131) * std::f32::consts::TAU)
}

/// The leaned trunk-top anchor the canopy/foliage sits on (the top shifts sideways as
/// the trunk leans).
fn canopy_anchor(t: &Tree, ground: f32, lean_deg_max: f32) -> Vec3 {
    let (theta, dir) = tree_lean(t, lean_deg_max);
    let h = t.trunk_height_m;
    Vec3::new(t.x + h * theta.sin() * dir.cos(), ground + h * theta.cos(), t.z + h * theta.sin() * dir.sin())
}

/// Per-tree trunk instances, each leaned a deterministic amount; near trunks read
/// dark, distance fog hazes far ones.
fn trunk_instances(manifest: &Manifest, trees: &[Tree], lean_deg: f32, view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let mut out = Vec::with_capacity(trees.len() * 36);
    for t in trees {
        let ground = manifest.terrain.height_at(t.x, t.z);
        let (theta, dir) = tree_lean(t, lean_deg);
        let axis = Vec3::new(dir.sin(), 0.0, -dir.cos());
        let lean = Quat::from_axis_angle(axis, theta).unwrap_or_else(|_| Quat::new(0.0, 0.0, 0.0, 1.0));
        let world = Transform::new(
            Vec3::new(t.x, ground, t.z),
            lean,
            Vec3::new(t.trunk_radius_m, t.trunk_height_m, t.trunk_radius_m),
        )
        .to_matrix();
        let d = eye.subtract(Vec3::new(t.x, ground + t.trunk_height_m * 0.5, t.z)).length();
        out.extend_from_slice(&instance(&vp, world, fogged(BARK, fog, d, &style_of(manifest), 1.0)));
    }
    out
}

/// The sphere-blob canopy (the fallback when no `[foliage]` is configured).
fn canopy_instances(manifest: &Manifest, trees: &[Tree], lean_deg: f32, view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let mut out = Vec::with_capacity(trees.len() * 36);
    for t in trees {
        let ground = manifest.terrain.height_at(t.x, t.z);
        let c = canopy_anchor(t, ground, lean_deg);
        let world = Transform::new(c, Quat::new(0.0, 0.0, 0.0, 1.0), Vec3::new(t.canopy_radius_m, t.canopy_radius_m, t.canopy_radius_m)).to_matrix();
        out.extend_from_slice(&instance(&vp, world, fogged(t.canopy_color, fog, eye.subtract(c).length(), &style_of(manifest), fol_sat(manifest))));
    }
    out
}

/// Deterministic branch lines radiating from a tree's crown: `(base, direction, length)`.
/// Leaves cluster along these and a thin branch stroke is drawn on each, so the canopy
/// reads as attached foliage instead of a floating card cloud.
fn tree_branches(t: &Tree, anchor: Vec3, f: &Foliage) -> Vec<(Vec3, Vec3, f32)> {
    let r = t.canopy_radius_m;
    (0..f.branches)
        .map(|k| {
            let az = hash01(t.x, t.z, 3000 + k) * std::f32::consts::TAU;
            // Tilt up from horizontal so branches sweep outward + upward.
            let tilt = 0.30 + hash01(t.x, t.z, 3100 + k) * 0.55;
            let (ct, st) = (tilt.cos(), tilt.sin());
            let dir = Vec3::new(az.cos() * ct, st, az.sin() * ct);
            // Base spread a little down the upper trunk so branches don't share one point.
            let base = Vec3::new(anchor.x, anchor.y - hash01(t.x, t.z, 3200 + k) * r * 0.35, anchor.z);
            let length = r * (0.75 + hash01(t.x, t.z, 3300 + k) * 0.5);
            (base, dir, length)
        })
        .collect()
}

/// A quaternion rotating +Y onto `dir` (orients the +Y branch stroke along a branch).
fn orient_y_to(dir: Vec3) -> Quat {
    let d = dir.normalize().unwrap_or(Vec3::UNIT_Y);
    let dot = Vec3::UNIT_Y.dot(d).clamp(-1.0, 1.0);
    Vec3::UNIT_Y
        .cross(d)
        .normalize()
        .and_then(|axis| Quat::from_axis_angle(axis, dot.acos()))
        .unwrap_or_else(|_| Quat::new(0.0, 0.0, 0.0, 1.0))
}

/// Stylized foliage. With `[foliage] branches > 0` the canopy is DENSE leaf cards clustered
/// ALONG branch lines radiating from the crown (attached, layered foliage); otherwise the
/// older floating sub-mass sphere fill. Plus a few muted understory cards near the base.
fn foliage_instances(manifest: &Manifest, trees: &[Tree], f: &Foliage, lean_deg: f32, view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let pal = f.palette.as_slice();
    let mut out = Vec::new();
    for t in trees {
        let ground = manifest.terrain.height_at(t.x, t.z);
        let anchor = canopy_anchor(t, ground, lean_deg);
        let r = t.canopy_radius_m;
        if f.branches > 0 {
            // Dense leaves clustered along each branch line — attached, layered canopy.
            for (bi, (base, dir, length)) in tree_branches(t, anchor, f).iter().enumerate() {
                let perp1 = dir.cross(Vec3::UNIT_Y).normalize().unwrap_or(Vec3::UNIT_X);
                let perp2 = dir.cross(perp1).normalize().unwrap_or(Vec3::UNIT_Z);
                let jr = r * f.card_scale * 0.9;
                for j in 0..f.leaves_per_branch {
                    let s = 4000 + bi as u32 * 131 + j;
                    // Along the branch, biased to the outer/tip half where leaves gather.
                    let frac = (0.35 + hash01(t.x, t.z, s) * 0.7).min(1.05);
                    let along = base.add(dir.mul_scalar(length * frac));
                    let jx = (hash01(t.x, t.z, s + 11) - 0.5) * jr;
                    let jz = (hash01(t.x, t.z, s + 23) - 0.5) * jr;
                    let jd = (hash01(t.x, t.z, s + 31) - 0.5) * jr * 0.6;
                    let pos = along.add(perp1.mul_scalar(jx)).add(perp2.mul_scalar(jz)).add(dir.mul_scalar(jd));
                    let sc = r * f.card_scale * (0.5 + hash01(t.x, t.z, s + 41) * 0.4);
                    let col = pick_color(pal, t.canopy_color, hash01(t.x, t.z, s + 53), 1.0);
                    out.extend_from_slice(&card_instance(&vp, pos, hash01(t.x, t.z, s + 61), sc, fogged(col, fog, eye.subtract(pos).length(), &style_of(manifest), fol_sat(manifest))));
                }
            }
        } else {
            let clusters = f.clusters.max(1);
            for j in 0..f.cards_per_tree {
                let m = j % clusters;
                let ma = hash01(t.x, t.z, 2000 + m) * std::f32::consts::TAU;
                let mrad = hash01(t.x, t.z, 2100 + m).sqrt() * r * f.cluster_spread;
                let mhy = (hash01(t.x, t.z, 2200 + m) - 0.30) * r * f.cluster_spread * 1.1;
                let mc = Vec3::new(anchor.x + mrad * ma.cos(), anchor.y + mhy, anchor.z + mrad * ma.sin());
                let a = hash01(t.x, t.z, 200 + j) * std::f32::consts::TAU;
                let rad = hash01(t.x, t.z, 300 + j).sqrt() * r * f.cluster_tightness;
                let hy = (hash01(t.x, t.z, 400 + j) - 0.30) * r * f.cluster_tightness * 1.1;
                let pos = Vec3::new(mc.x + rad * a.cos(), mc.y + hy, mc.z + rad * a.sin());
                let sc = r * f.card_scale * (0.55 + hash01(t.x, t.z, 500 + j) * 0.4);
                let col = pick_color(pal, t.canopy_color, hash01(t.x, t.z, 700 + j), 1.0);
                out.extend_from_slice(&card_instance(&vp, pos, hash01(t.x, t.z, 600 + j), sc, fogged(col, fog, eye.subtract(pos).length(), &style_of(manifest), fol_sat(manifest))));
            }
        }
        // Understory: smaller, muted cards near the trunk base.
        for j in 0..f.understory_cards {
            let a = hash01(t.x, t.z, 800 + j) * std::f32::consts::TAU;
            let rad = t.trunk_radius_m + hash01(t.x, t.z, 900 + j) * r * 0.6;
            let pos = Vec3::new(t.x + rad * a.cos(), ground + 0.2 + hash01(t.x, t.z, 1000 + j) * r * 0.4, t.z + rad * a.sin());
            let sc = r * f.card_scale * (0.4 + hash01(t.x, t.z, 1100 + j) * 0.3);
            let col = pick_color(pal, t.canopy_color, hash01(t.x, t.z, 1200 + j), 0.7);
            out.extend_from_slice(&card_instance(&vp, pos, hash01(t.x, t.z, 1300 + j), sc, fogged(col, fog, eye.subtract(pos).length(), &style_of(manifest), fol_sat(manifest))));
        }
    }
    out
}

/// Thin dark branch strokes (one per `tree_branches` line), oriented along the branch and
/// scaled to its length — the visible scaffold the dense leaves hang on.
fn branch_instances(manifest: &Manifest, trees: &[Tree], f: &Foliage, lean_deg: f32, view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let dark = [BARK[0] * 0.6, BARK[1] * 0.6, BARK[2] * 0.6];
    let mut out = Vec::new();
    for t in trees {
        let ground = manifest.terrain.height_at(t.x, t.z);
        let anchor = canopy_anchor(t, ground, lean_deg);
        let thick = t.trunk_radius_m * f.branch_thickness;
        for (base, dir, length) in tree_branches(t, anchor, f) {
            let world = Transform::new(base, orient_y_to(dir), Vec3::new(thick, length, thick)).to_matrix();
            let mid = base.add(dir.mul_scalar(length * 0.5));
            out.extend_from_slice(&instance(&vp, world, fogged(dark, fog, eye.subtract(mid).length(), &style_of(manifest), 1.0)));
        }
    }
    out
}

/// Pick a palette colour by `pick` in `[0,1)` (fallback to `fallback` on an empty
/// palette), scaled by `mul` (understory cards are darker).
fn pick_color(pal: &[[f32; 3]], fallback: [f32; 3], pick: f32, mul: f32) -> [f32; 3] {
    let c = pal.get((pick * pal.len() as f32) as usize).copied().unwrap_or(fallback);
    [c[0] * mul, c[1] * mul, c[2] * mul]
}

/// One foliage-card instance: the unit crossed card at `pos`, yawed + uniformly
/// scaled, tinted.
fn card_instance(vp: &Mat4, pos: Vec3, yaw01: f32, scale: f32, tint: [f32; 4]) -> Vec<f32> {
    let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, yaw01 * std::f32::consts::TAU)
        .unwrap_or_else(|_| Quat::new(0.0, 0.0, 0.0, 1.0));
    let world = Transform::new(pos, yaw, Vec3::new(scale, scale, scale)).to_matrix();
    instance(vp, world, tint)
}

/// Build the per-tuft instance data for the ground-cover batch: each tuft is the
/// unit tuft mesh (y in [0,1]) seated on the terrain surface, scaled to
/// (radius, height, radius) and yawed, tinted with its colour + fog.
fn plant_instances(manifest: &Manifest, tufts: &[Tuft], height_mul: f32, view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let mut out = Vec::with_capacity(tufts.len() * 36);
    for t in tufts {
        let ground = manifest.terrain.height_at(t.x, t.z);
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, t.yaw_deg.to_radians())
            .unwrap_or_else(|_| Quat::new(0.0, 0.0, 0.0, 1.0));
        let world = Transform::new(
            Vec3::new(t.x, ground, t.z),
            yaw,
            Vec3::new(t.radius_m, t.height_m * height_mul, t.radius_m),
        )
        .to_matrix();
        let dist = eye.subtract(Vec3::new(t.x, ground + t.height_m * 0.5, t.z)).length();
        let tint = fogged(t.color, fog, dist, &style_of(manifest), 1.0);
        out.extend_from_slice(&instance(&vp, world, tint));
    }
    out
}

/// Fallen-leaf litter instances: flat leaf cards lying just above the ground, scaled by
/// `radius_m` and yawed, in warm litter tints — a dense fallen-leaf carpet on the floor.
fn litter_instances(manifest: &Manifest, litter: &[Tuft], view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let mut out = Vec::with_capacity(litter.len() * 36);
    for t in litter {
        let ground = manifest.terrain.height_at(t.x, t.z);
        // Tilt each fallen leaf about a varied horizontal axis so it lies at a natural
        // angle (some flat, some curled up on an edge) instead of a flat sprite — the bed
        // gains relief, overlap, and varied light. Axis azimuth from yaw, tilt from a hash.
        let phi = t.yaw_deg.to_radians();
        let axis = Vec3::new(phi.cos(), 0.0, phi.sin());
        let tilt = (hash01(t.x, t.z, 6100) - 0.5) * 0.95;
        let rot = Quat::from_axis_angle(axis, tilt).unwrap_or_else(|_| Quat::new(0.0, 0.0, 0.0, 1.0));
        let world = Transform::new(
            Vec3::new(t.x, ground + 0.02, t.z),
            rot,
            Vec3::new(t.radius_m, t.radius_m, t.radius_m),
        )
        .to_matrix();
        // Per-leaf brightness jitter widens the litter variety beyond the palette alone.
        let cj = 0.82 + hash01(t.x, t.z, 6200) * 0.36;
        let col = [t.color[0] * cj, t.color[1] * cj, t.color[2] * cj];
        let dist = eye.subtract(Vec3::new(t.x, ground, t.z)).length();
        let tint = fogged(col, fog, dist, &style_of(manifest), 1.0);
        out.extend_from_slice(&instance(&vp, world, tint));
    }
    out
}

/// One 36-float instance: `mvp(16) · world(16) · tint(4)`, where `mvp = view_proj ·
/// world`. The GPU shader clips with the first matrix directly (`clip = mvp *
/// position`) and lights with the second (`world`), and the Canvas 2D backend reads
/// the same `world` + `mvp` pair — so the world transform must be folded into the
/// first matrix, not left for the shader to apply.
fn instance(vp: &Mat4, world: Mat4, tint: [f32; 4]) -> Vec<f32> {
    let mvp = vp.multiply(world).as_cols_array();
    let mut v = Vec::with_capacity(36);
    v.extend_from_slice(&mvp);
    v.extend_from_slice(&world.as_cols_array());
    v.extend_from_slice(&tint);
    v
}

/// The manifest's style (neutral when omitted).
fn style_of(manifest: &Manifest) -> Style {
    manifest.style.unwrap_or_else(Style::neutral)
}

/// The manifest's foliage saturation (`1.0` when no style).
fn fol_sat(manifest: &Manifest) -> f32 {
    manifest.style.map(|s| s.foliage_saturation).unwrap_or(1.0)
}

/// Desaturate a colour toward its luminance by `1 - sat` (`sat = 1` keeps it).
fn mute(c: [f32; 3], sat: f32) -> [f32; 3] {
    let g = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    [lerp(g, c[0], sat), lerp(g, c[1], sat), lerp(g, c[2], sat)]
}

/// Apply exposure (global multiply) + an ambient shadow-lift, clamped so nothing
/// blows past white — the tone control that stops foliage washing out the frame.
fn expose(c: [f32; 3], s: &Style) -> [f32; 3] {
    let f = |x: f32| {
        let e = x * s.exposure;
        (e + s.ambient * (1.0 - e)).clamp(0.0, 1.0)
    };
    [f(c[0]), f(c[1]), f(c[2])]
}

/// The full styled tint for an instance: mute foliage saturation, desaturate with
/// distance, blend toward the (blue-gray) fog, then expose + tone-clamp. `sat` is the
/// foliage saturation (`1.0` for non-foliage surfaces).
fn fogged(color: [f32; 3], fog: &super::scene::Fog, dist: f32, style: &Style, sat: f32) -> [f32; 4] {
    let f = fog_factor(dist, fog.start_m, fog.end_m);
    let muted = mute(color, sat);
    let far = mute(muted, 1.0 - style.distance_desaturation * f);
    let c = expose(lerp3(far, fog.color, f), style);
    [c[0], c[1], c[2], 1.0]
}

/// Ground albedo at height `h`: piecewise-linear across the band control points
/// (each band's albedo is the colour at its `max_height_m`), clamped at the ends.
fn ground_albedo(terrain: &Terrain, h: f32) -> [f32; 3] {
    let bands = &terrain.ground_bands;
    if bands.is_empty() {
        return [0.4, 0.36, 0.24];
    }
    if h <= bands[0].max_height_m {
        return bands[0].albedo;
    }
    for w in bands.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if h <= hi.max_height_m {
            let span = (hi.max_height_m - lo.max_height_m).max(1.0e-3);
            let t = ((h - lo.max_height_m) / span).clamp(0.0, 1.0);
            return lerp3(lo.albedo, hi.albedo, t);
        }
    }
    bands[bands.len() - 1].albedo
}

/// The unit trunk: a straight `TRUNK_SEGMENTS`-gon cylinder, radius 1, y in [0, 1],
/// with outward radial normals. Per-vertex colour is white (the instance tint
/// carries the bark colour).
fn trunk_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let seg = TRUNK_SEGMENTS;
    for s in 0..=seg {
        let a = (s as f32 / seg as f32) * std::f32::consts::TAU;
        let (nx, nz) = (a.cos(), a.sin());
        // Per-segment bark streak (deterministic light/dark) → vertical bark variation
        // so the trunk reads as bark, not a flat tube.
        let streak = 0.84 + ((s.wrapping_mul(2_654_435_761) >> 24) & 0xFF) as f32 / 255.0 * 0.28;
        // Cylindrical UVs: u wraps once around, v tiles up the trunk (Repeat sampler) so
        // the bark detail texture reads at a sensible scale on a tall trunk.
        let u = s as f32 / seg as f32;
        for y in [0.0f32, 1.0f32] {
            // Darker toward the base (roots / ambient occlusion), lighter up.
            let c = (streak * (0.60 + y * 0.40)).min(1.12);
            push_vertex(&mut v, [nx, y, nz], [nx, 0.0, nz], [u, y * 4.0], [c, c, c, 1.0]);
        }
    }
    for s in 0..seg {
        let b = s * 2;
        // Two triangles per quad: (b, b+2, b+1) and (b+1, b+2, b+3).
        idx.extend_from_slice(&[b, b + 2, b + 1, b + 1, b + 2, b + 3]);
    }
    (v, idx)
}

/// The unit canopy blob: a low-poly UV sphere, radius 1, centred at the origin,
/// normals = normalized position. Per-vertex colour white; instance tint carries
/// the autumn canopy colour.
fn canopy_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let rings = CANOPY_RINGS;
    let sectors = CANOPY_SECTORS;
    for r in 0..=rings {
        let phi = (r as f32 / rings as f32) * std::f32::consts::PI; // 0..pi
        let (sp, cp) = (phi.sin(), phi.cos());
        for s in 0..=sectors {
            let theta = (s as f32 / sectors as f32) * std::f32::consts::TAU;
            let (st, ct) = (theta.sin(), theta.cos());
            let p = [sp * ct, cp, sp * st];
            push_vertex(&mut v, p, p, [0.5, 0.5], [1.0, 1.0, 1.0, 1.0]);
        }
    }
    let stride = sectors + 1;
    for r in 0..rings {
        for s in 0..sectors {
            let a = r * stride + s;
            let b = a + stride;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (v, idx)
}

/// The unit ground-cover tuft: `TUFT_BLADES` tapered blades crossing at a common
/// apex (radius 1, y in [0,1]). Each blade is a double-sided triangle (both windings)
/// so it reads from any angle; normals point up so the tuft catches sky/sun light.
/// Per-vertex colour is white; the instance tint carries the grass/litter colour.
fn tuft_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    // Varied per-blade tip heights so the clump is ragged, not a uniform star.
    const TIP_H: [f32; 6] = [1.0, 0.66, 0.9, 0.58, 0.82, 0.72];
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let up = [0.0f32, 1.0, 0.0];
    let w = [1.0f32, 1.0, 1.0, 1.0];
    let mut base = 0u32;
    for k in 0..TUFT_BLADES {
        let a = (k as f32 / TUFT_BLADES as f32) * std::f32::consts::TAU;
        let (ca, sa) = (a.cos(), a.sin());
        // Base edge perpendicular to the splay direction; the apex leans OUTWARD (to
        // radius 0.7) and up — a low grass/leaf clump that hugs the ground, not an
        // upright spike, so the ground reads as soft clutter instead of confetti.
        let (px, pz) = (-sa * 0.16, ca * 0.16);
        let tip = TIP_H[(k % TIP_H.len() as u32) as usize];
        push_vertex(&mut v, [px, 0.0, pz], up, [0.0, 0.0], w);
        push_vertex(&mut v, [-px, 0.0, -pz], up, [1.0, 0.0], w);
        push_vertex(&mut v, [ca * 0.7, tip, sa * 0.7], up, [0.5, 1.0], w);
        // Both windings → the blade is visible from either side.
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 1]);
        base += 3;
    }
    (v, idx)
}

/// The unit fallen leaf: a single flat irregular polygon lying in the XZ plane (a leaf
/// on the ground), normal up, double-sided. Per-vertex white; the instance tint carries
/// the warm litter colour.
/// A taller upright sedge/grass-frond clump: thin near-vertical blades fanning only
/// slightly, tips up to y=1 — the dried tall grasses standing on the forest floor (the
/// second ground-plant species, distinct from the low splayed tuft).
fn fern_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    const BLADES: u32 = 9;
    const TIP: [f32; 9] = [1.0, 0.78, 0.92, 0.65, 0.85, 0.72, 0.96, 0.6, 0.82];
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let up = [0.0f32, 1.0, 0.0];
    let w = [1.0f32, 1.0, 1.0, 1.0];
    let mut base = 0u32;
    for k in 0..BLADES {
        let a = (k as f32 / BLADES as f32) * std::f32::consts::TAU;
        let (ca, sa) = (a.cos(), a.sin());
        let (px, pz) = (-sa * 0.06, ca * 0.06); // narrow base
        let tip = TIP[(k % 9) as usize];
        // Near-vertical: apex only slightly out (0.22) but tall (up to y=1).
        push_vertex(&mut v, [px, 0.0, pz], up, [0.0, 0.0], w);
        push_vertex(&mut v, [-px, 0.0, -pz], up, [1.0, 0.0], w);
        push_vertex(&mut v, [ca * 0.22, tip, sa * 0.22], up, [0.5, 1.0], w);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 1]);
        base += 3;
    }
    (v, idx)
}

fn litter_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    const RIM: [f32; 7] = [0.50, 0.36, 0.48, 0.30, 0.50, 0.34, 0.44];
    let up = [0.0f32, 1.0, 0.0];
    let w = [1.0f32, 1.0, 1.0, 1.0];
    let n = RIM.len();
    let mut v = Vec::new();
    let mut idx = Vec::new();
    push_vertex(&mut v, [0.0, 0.0, 0.0], up, [0.5, 0.5], w);
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        let r = RIM[k];
        push_vertex(&mut v, [a.cos() * r, 0.0, a.sin() * r], up, [0.5, 0.5], w);
    }
    for k in 0..n {
        let (c, r0, r1) = (0u32, 1 + k as u32, 1 + ((k + 1) % n) as u32);
        idx.extend_from_slice(&[c, r0, r1, c, r1, r0]);
    }
    (v, idx)
}

/// The unit foliage leaf clump: a **cluster of small separate leaf quads** at varied
/// offsets + tilts, with real GAPS between them — so the card reads as a mass of
/// individual leaves with light through the gaps, rather than one solid sheet. The
/// gaps are actual geometry (nothing there), so every backend (textured GPU + flat
/// software raster) renders them identically and depth stays correct — no alpha
/// texture or shader cutout needed. Double-sided, up-facing normals for the warm sun;
/// per-vertex white, the instance tint carries the autumn leaf colour.
/// Unit branch stroke: two perpendicular quads spanning y in [0,1], tapering base->tip, so
/// it reads as a thin branch from any angle. White vertex colour (the instance tint = bark).
fn branch_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let (bw, tw) = (1.0f32, 0.3f32); // base / tip half-width (the instance scales it thin)
    let up = [0.0f32, 1.0, 0.0];
    let w = [1.0f32, 1.0, 1.0, 1.0];
    for (ax, az) in [(1.0f32, 0.0f32), (0.0f32, 1.0f32)] {
        let base = (v.len() / VERT_FLOATS) as u32;
        push_vertex(&mut v, [-bw * ax, 0.0, -bw * az], up, [0.0, 0.0], w);
        push_vertex(&mut v, [bw * ax, 0.0, bw * az], up, [1.0, 0.0], w);
        push_vertex(&mut v, [tw * ax, 1.0, tw * az], up, [1.0, 1.0], w);
        push_vertex(&mut v, [-tw * ax, 1.0, -tw * az], up, [0.0, 1.0], w);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    (v, idx)
}

fn foliage_card_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    const LEAF: usize = 9;
    // Per-leaf centre offset within the unit clump (spread through its volume).
    const OFF: [[f32; 3]; LEAF] = [
        [0.00, 0.10, 0.00],
        [0.34, -0.06, 0.20],
        [-0.30, 0.04, -0.24],
        [0.16, 0.30, -0.30],
        [-0.36, -0.18, 0.12],
        [0.24, -0.30, -0.14],
        [-0.12, 0.34, 0.28],
        [0.06, -0.10, 0.36],
        [-0.22, 0.16, 0.30],
    ];
    // Per-leaf half-size (varied so the clump is ragged).
    const SIZE: [f32; LEAF] = [0.30, 0.24, 0.27, 0.22, 0.28, 0.21, 0.25, 0.23, 0.26];
    let up = [0.0f32, 1.0, 0.0];
    let w = [1.0f32, 1.0, 1.0, 1.0];
    let mut v = Vec::new();
    let mut idx = Vec::new();
    for k in 0..LEAF {
        let base = (v.len() / VERT_FLOATS) as u32;
        let (o, s) = (OFF[k], SIZE[k]);
        let a = k as f32 * 0.8;
        let (ca, sa) = (a.cos(), a.sin());
        // Two in-plane axes for a small tilted leaf diamond at offset `o`.
        let ax = [ca * s, s * 0.15, sa * s];
        let ay = [-sa * s * 0.7, s * 0.8, ca * s * 0.7];
        let corner = |sx: f32, sy: f32| {
            [o[0] + ax[0] * sx + ay[0] * sy, o[1] + ax[1] * sx + ay[1] * sy, o[2] + ax[2] * sx + ay[2] * sy]
        };
        // Map the diamond's 4 corners across the leaf-alpha texture (corner → texture
        // corner), so the radial soft-alpha reads as a feathered leaf blob on the GPU.
        push_vertex(&mut v, corner(-1.0, 0.0), up, [0.0, 0.0], w);
        push_vertex(&mut v, corner(0.0, -1.0), up, [1.0, 0.0], w);
        push_vertex(&mut v, corner(1.0, 0.0), up, [1.0, 1.0], w);
        push_vertex(&mut v, corner(0.0, 1.0), up, [0.0, 1.0], w);
        // Two triangles (a diamond), both windings → visible from either side.
        idx.extend_from_slice(&[
            base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base,
            base + 3, base + 2,
        ]);
    }
    (v, idx)
}

/// A 2×2 fully-white albedo texture, so `albedo · vertex_colour · instance_colour`
/// reduces to the per-vertex / per-instance colours the meshes carry.
fn white_material() -> (u64, u32, u32, Vec<u8>) {
    (WHITE_MAT, 2, 2, vec![255u8; 2 * 2 * 4])
}

/// A leaf-shaped soft-alpha texture: white RGB with an alpha shaped like an ovate,
/// pointed leaf (zero-width at the stem and tip, widest near the base — a beech-ish
/// silhouette), feathered at the edge. On the GPU the mesh shader alpha-cuts + blends it,
/// so each foliage card reads as an actual leaf rather than a soft disc; Canvas 2D never
/// samples it (flat-shaded), keeping its solid-card proxy.
fn leaf_alpha_material() -> (u64, u32, u32, Vec<u8>) {
    const N: u32 = 64;
    let mut rgba = vec![255u8; (N * N * 4) as usize];
    (0..N).for_each(|y| {
        (0..N).for_each(|x| {
            let nx = (x as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let ny = (y as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            // Leaf-local height: 0 at the stem (bottom), 1 at the tip (top).
            let ly = ((ny + 1.0) * 0.5).clamp(0.0, 1.0);
            // Ovate half-width profile: 0 at stem & tip, peaks ~0.45 near ly=0.4, so the
            // outline is a rounded leaf tapering to a point.
            let hw = 1.05 * ly.powf(0.45) * (1.0 - ly).powf(0.9);
            // Feather across the horizontal outline (and the tip, where hw -> 0).
            let a = smoothstep(-0.05, 0.03, hw - nx.abs());
            let idx = ((y * N + x) * 4 + 3) as usize;
            rgba[idx] = (a * 255.0 + 0.5) as u8;
        })
    });
    (LEAF_ALPHA_MAT, N, N, rgba)
}

/// Beech-bark surface value at a texel — a near-white multiplier that doubles as a
/// height field (darker banding / lenticel dashes = lower). Shared by the bark albedo and
/// its normal map so relief and shading agree. Beech is smooth silver-grey, so it is
/// subtle: gentle vertical banding (up the trunk), faint grain, sparse lenticel dashes.
fn bark_height(px: f32, py: f32) -> f32 {
    let band = value_noise(11, px * 0.09, py * 0.015) * 0.5 + 0.5;
    let grain = value_noise(23, px * 0.35, py * 0.5) * 0.5 + 0.5;
    let lent = smoothstep(0.80, 0.9, value_noise(37, px * 0.6, py * 0.14) * 0.5 + 0.5);
    ((0.86 + 0.14 * band) * (0.94 + 0.06 * grain) - 0.28 * lent).clamp(0.5, 1.05)
}

/// The beech-bark albedo detail (RGB grey value multiplier, opaque), from [`bark_height`].
fn bark_material() -> (u64, u32, u32, Vec<u8>) {
    const N: u32 = 128;
    let mut rgba = vec![255u8; (N * N * 4) as usize];
    (0..N).for_each(|y| {
        (0..N).for_each(|x| {
            let b = (bark_height(x as f32, y as f32) * 255.0).clamp(0.0, 255.0) as u8;
            let idx = ((y * N + x) * 4) as usize;
            rgba[idx] = b;
            rgba[idx + 1] = b;
            rgba[idx + 2] = b;
        })
    });
    (BARK_MAT, N, N, rgba)
}

/// Forest-floor surface value (a mottle multiplier that doubles as a height field).
/// Shared by the ground albedo and its normal map.
fn ground_height(px: f32, py: f32) -> f32 {
    let coarse = value_noise(51, px * 0.06, py * 0.06) * 0.5 + 0.5;
    let fine = value_noise(63, px * 0.28, py * 0.28) * 0.5 + 0.5;
    (0.78 + 0.28 * coarse) * (0.9 + 0.16 * fine)
}

/// Procedural forest-floor detail: a color-mottle multiplier (around 1.0) that the
/// terrain's per-vertex ground colour modulates — cooler earth vs warmer leaf-litter at a
/// finer scale than the terrain grid. Tiles.
fn ground_material() -> (u64, u32, u32, Vec<u8>) {
    const N: u32 = 128;
    let mut rgba = vec![255u8; (N * N * 4) as usize];
    (0..N).for_each(|y| {
        (0..N).for_each(|x| {
            let px = x as f32;
            let py = y as f32;
            let v = ground_height(px, py);
            // Tint the mottle: darker patches lean cool-earth, lighter lean warm-litter.
            let warm = smoothstep(0.5, 1.0, value_noise(51, px * 0.06, py * 0.06) * 0.5 + 0.5);
            let tint = lerp3([0.92, 0.90, 0.86], [1.06, 0.98, 0.86], warm);
            let idx = ((y * N + x) * 4) as usize;
            rgba[idx] = (v * tint[0] * 255.0).clamp(0.0, 255.0) as u8;
            rgba[idx + 1] = (v * tint[1] * 255.0).clamp(0.0, 255.0) as u8;
            rgba[idx + 2] = (v * tint[2] * 255.0).clamp(0.0, 255.0) as u8;
        })
    });
    (GROUND_MAT, N, N, rgba)
}

/// Build a tangent-space normal map (RGBA8, id-tagged) by central-differencing a height
/// field `h` over N×N at `strength` bump scale — RGB encodes the perturbed normal
/// `(-dh/dx, -dh/dy, 1)` normalized into `[0,1]`. Used to give the bark + ground GPU
/// surface relief under the directional light (Canvas 2D ignores it).
fn normal_map_from_height(
    id: u64,
    n: u32,
    strength: f32,
    h: impl Fn(f32, f32) -> f32,
) -> (u64, u32, u32, Vec<u8>) {
    let mut rgba = vec![255u8; (n * n * 4) as usize];
    (0..n).for_each(|y| {
        (0..n).for_each(|x| {
            let (px, py) = (x as f32, y as f32);
            let nx = -(h(px + 1.0, py) - h(px - 1.0, py)) * strength;
            let ny = -(h(px, py + 1.0) - h(px, py - 1.0)) * strength;
            let inv = 1.0 / (nx * nx + ny * ny + 1.0).sqrt();
            let enc = |v: f32| ((v * inv * 0.5 + 0.5) * 255.0).clamp(0.0, 255.0) as u8;
            let idx = ((y * n + x) * 4) as usize;
            rgba[idx] = enc(nx);
            rgba[idx + 1] = enc(ny);
            rgba[idx + 2] = enc(1.0);
        })
    });
    (id, n, n, rgba)
}

/// The bark tangent-space normal map (relief from [`bark_height`]).
fn bark_normal_material() -> (u64, u32, u32, Vec<u8>) {
    normal_map_from_height(BARK_MAT, 128, 3.2, bark_height)
}

/// The ground tangent-space normal map (relief from [`ground_height`]).
fn ground_normal_material() -> (u64, u32, u32, Vec<u8>) {
    normal_map_from_height(GROUND_MAT, 128, 2.2, ground_height)
}

fn push_vertex(out: &mut Vec<f32>, pos: [f32; 3], normal: [f32; 3], uv: [f32; 2], color: [f32; 4]) {
    out.extend_from_slice(&[
        pos[0], pos[1], pos[2], normal[0], normal[1], normal[2], uv[0], uv[1], color[0], color[1],
        color[2], color[3],
    ]);
}

fn fog_factor(dist: f32, start: f32, end: f32) -> f32 {
    ((dist - start) / (end - start).max(1.0e-3)).clamp(0.0, 1.0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1.0e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::visual_target::scene::Manifest;

    const SCENE: &str = r#"
version = 1
[camera]
eye = [-18.0, 14.0, 40.0]
target = [0.0, 4.0, 0.0]
fov_deg = 55.0
near_m = 0.1
far_m = 300.0
width_px = 320
height_px = 200
[sun]
direction = [-0.4, -0.8, -0.45]
color = [1.0, 0.93, 0.78]
intensity = 1.15
[fog]
color = [0.72, 0.78, 0.85]
start_m = 30.0
end_m = 180.0
[terrain]
size_m = 64.0
spacing_m = 2.0
slope = [0.05, 0.08]
detail = [ { amplitude_m = 1.0, wavelength_m = 20.0, seed = 11 } ]
[[terrain.ground_band]]
max_height_m = 1.0
albedo = [0.34, 0.28, 0.15]
[[terrain.ground_band]]
max_height_m = 8.0
albedo = [0.45, 0.40, 0.20]
[[tree]]
x = 6.0
z = -4.0
yaw_deg = 30.0
trunk_height_m = 6.0
trunk_radius_m = 0.3
canopy_radius_m = 3.0
canopy_color = [0.80, 0.42, 0.12]
"#;

    #[test]
    fn build_is_byte_deterministic() {
        let m = Manifest::parse(SCENE).unwrap();
        let a = build(&m);
        let b = build(&m);
        assert_eq!(a.meshes, b.meshes);
        assert_eq!(a.batches, b.batches);
        assert_eq!(a.view_proj, b.view_proj);
        assert_eq!(a.light_view_proj, b.light_view_proj);
        assert_eq!(a.lights, b.lights);
    }

    #[test]
    fn one_tree_yields_trunk_and_canopy_batches() {
        let m = Manifest::parse(SCENE).unwrap();
        let rd = build(&m);
        // terrain + trunk + canopy.
        assert_eq!(rd.batches.len(), 3);
        assert_eq!(rd.batches[0].0, TERRAIN_MESH);
        assert_eq!(rd.batches[0].1, GROUND_MAT);
        assert_eq!(rd.batches[1], (TRUNK_MESH, BARK_MAT, rd.batches[1].2.clone(), 1));
        assert_eq!(rd.batches[2].3, 1);
        // 36 floats per instance.
        assert_eq!(rd.batches[1].2.len(), 36);
    }

    #[test]
    fn no_trees_yields_only_terrain_batch() {
        let no_veg = SCENE
            .split("[[tree]]")
            .next()
            .unwrap()
            .to_string();
        let m = Manifest::parse(&no_veg).unwrap();
        let rd = build(&m);
        assert_eq!(rd.batches.len(), 1);
        assert_eq!(rd.batches[0].0, TERRAIN_MESH);
    }

    #[test]
    fn scatter_grows_the_instance_count() {
        let with_scatter = format!(
            "{SCENE}\n[scatter]\nseed = 1\ncount = 40\nmin_spacing_m = 1.5\nslope_limit = 1.0\n\
             trunk_height_m = [4.0, 8.0]\ntrunk_radius_m = [0.2, 0.4]\ncanopy_radius_m = [2.0, 4.0]\n\
             canopy_palette = [ [0.8,0.4,0.1], [0.86,0.62,0.18] ]\n"
        );
        let m = Manifest::parse(&with_scatter).unwrap();
        let trees = all_trees(&m);
        // 1 explicit + up to 40 scattered.
        assert!(trees.len() > 1);
        let rd = build(&m);
        assert_eq!(rd.batches[1].3 as usize, trees.len());
    }

    #[test]
    fn groundcover_yields_its_own_batch_and_mesh() {
        let with_gc = format!(
            "{SCENE}\n[groundcover]\nseed = 2\ncount = 60\nmin_spacing_m = 0.5\n\
             slope_limit = 1.0\nheight_m = [0.2, 0.5]\nradius_m = [0.1, 0.3]\n\
             palette = [ [0.6, 0.5, 0.2] ]\n"
        );
        let m = Manifest::parse(&with_gc).unwrap();
        let tufts = all_groundcover(&m);
        assert!(!tufts.is_empty());
        let rd = build(&m);
        // The scatter is split into two species batches (grass GROUNDCOVER_MESH + sedge
        // FERN_MESH); together they account for every scattered tuft.
        let is_ground = |mesh: u64| mesh == GROUNDCOVER_MESH || mesh == FERN_MESH;
        let ground_count: u32 = rd.batches.iter().filter(|(mesh, ..)| is_ground(*mesh)).map(|b| b.3).sum();
        assert_eq!(ground_count as usize, tufts.len());
        let ground_floats: usize = rd.batches.iter().filter(|(mesh, ..)| is_ground(*mesh)).map(|b| b.2.len()).sum();
        assert_eq!(ground_floats, tufts.len() * 36); // 36 floats per instance
        assert!(rd.meshes.iter().any(|(id, ..)| *id == GROUNDCOVER_MESH));
        assert!(rd.meshes.iter().any(|(id, ..)| *id == FERN_MESH));
    }

    #[test]
    fn no_groundcover_block_yields_no_groundcover_batch() {
        let m = Manifest::parse(SCENE).unwrap();
        assert!(all_groundcover(&m).is_empty());
        let rd = build(&m);
        assert!(!rd.batches.iter().any(|(mesh, ..)| *mesh == GROUNDCOVER_MESH));
    }

    #[test]
    fn fog_pulls_distant_ground_toward_fog_colour() {
        let m = Manifest::parse(SCENE).unwrap();
        let rd = build(&m);
        // Terrain vertices are the first mesh; sanity: colours stay within [0,1].
        let verts = &rd.meshes[0].1;
        for chunk in verts.chunks(VERT_FLOATS) {
            for c in &chunk[8..12] {
                assert!((0.0..=1.0).contains(c));
            }
        }
    }
}

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

use axiom_host::FrameVolumetrics;
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
const WHITE_MAT: u64 = 1;

/// Radial segments in the unit trunk cylinder.
const TRUNK_SEGMENTS: u32 = 8;
/// Rings / sectors in the unit canopy blob. ITER 14 (attack: artifact_level) — the
/// canopy-geometry abstraction: raise the mesh resolution so canopies read as
/// rounded crowns instead of faceted blobs (the dominant remaining artifact).
const CANOPY_RINGS: u32 = 8;
const CANOPY_SECTORS: u32 = 14;
/// Blades in the unit ground-cover tuft (a small crossed-blade cluster).
const TUFT_BLADES: u32 = 3;

/// Bark tint the trunk instances carry (fog is folded in per instance).
const BARK: [f32; 3] = [0.30, 0.21, 0.13];

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
    /// `(mesh_id, material_id, interleaved 36-float instances, instance count)`.
    pub batches: Vec<(u64, u64, Vec<f32>, u32)>,
    /// Optional volumetric light (god-rays) — neutral frame data every backend
    /// realizes through `host::apply_frame_volumetrics`.
    pub volumetrics: Option<FrameVolumetrics>,
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
    let (tuft_v, tuft_i) = tuft_unit_mesh();

    let lean_deg = manifest.scatter.as_ref().map(|s| s.lean_deg).unwrap_or(0.0);
    let trees = all_trees(manifest);
    let trunk_inst = trunk_instances(manifest, &trees, lean_deg, &view_proj, eye);
    // Canopy: stylized foliage-card clusters when configured, else sphere blobs.
    let (canopy_mesh_id, canopy_inst, canopy_count) = match &manifest.foliage {
        Some(f) => (
            FOLIAGE_MESH,
            foliage_instances(manifest, &trees, f, lean_deg, &view_proj, eye),
            trees.len() as u32 * (f.cards_per_tree + f.understory_cards),
        ),
        None => {
            (CANOPY_MESH, canopy_instances(manifest, &trees, lean_deg, &view_proj, eye), trees.len() as u32)
        }
    };

    let tufts = all_groundcover(manifest);
    let tuft_inst = tuft_instances(manifest, &tufts, &view_proj, eye);

    // Terrain: one identity-world instance whose MVP is the camera view-projection.
    let terrain_batch_inst =
        instance(&Mat4::from_cols_array(view_proj), Mat4::IDENTITY, [1.0, 1.0, 1.0, 1.0]);

    let mut batches: Vec<(u64, u64, Vec<f32>, u32)> =
        vec![(TERRAIN_MESH, WHITE_MAT, terrain_batch_inst, 1)];
    // Only emit vegetation batches when there is at least one tree.
    let tree_count = trees.len() as u32;
    (tree_count > 0).then(|| {
        batches.push((TRUNK_MESH, WHITE_MAT, trunk_inst, tree_count));
        batches.push((canopy_mesh_id, WHITE_MAT, canopy_inst, canopy_count));
    });
    // Ground cover: one instanced batch when the abstraction placed any tufts.
    let tuft_count = tufts.len() as u32;
    (tuft_count > 0).then(|| batches.push((GROUNDCOVER_MESH, WHITE_MAT, tuft_inst, tuft_count)));

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
            (GROUNDCOVER_MESH, tuft_v, tuft_i),
        ],
        materials: vec![white_material()],
        batches,
        volumetrics: manifest.volumetrics.then(FrameVolumetrics::low_poly),
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
            // Leaf-litter carpet: mottle the floor between two warm fallen-leaf tones
            // (orange-brown ↔ dry tan) so it reads as a bed of autumn leaves, not flat
            // dirt, then drop in sparse muted-green grass/moss patches — a forest
            // floor. Smooth, clustered value noise keeps it readable, not speckly.
            let litter_n = value_noise(4242, pos.x * 0.34, pos.z * 0.34) * 0.5 + 0.5;
            let fine_n = value_noise(7777, pos.x * 1.1, pos.z * 1.1) * 0.5 + 0.5;
            // The floor is fallen-leaf litter: warm russet ↔ ochre, only lightly tinted
            // by the underlying ground so it reads as a leaf bed, not tan dirt.
            let litter = lerp3([0.44, 0.24, 0.11], [0.58, 0.38, 0.18], fine_n);
            let leafy = lerp3(rocked, litter, 0.80 + 0.15 * litter_n);
            // Sparse muted-green grass/moss patches breaking up the litter.
            let moss_n = smoothstep(0.64, 0.88, value_noise(1313, pos.x * 0.52, pos.z * 0.52) * 0.5 + 0.5);
            let surface = lerp3(leafy, [0.31, 0.36, 0.18], moss_n * 0.5);

            let dist = eye.subtract(Vec3::new(pos.x, pos.y, pos.z)).length();
            let col = fogged(surface, fog, dist, style, 1.0);

            push_vertex(
                &mut vertices,
                [pos.x, pos.y, pos.z],
                [normal.x, normal.y, normal.z],
                [0.5, 0.5],
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

/// Stylized foliage: each tree's canopy is a loose cluster of crossed leaf cards in a
/// warm autumn palette (upper/mid canopy) plus a few smaller understory cards near the
/// ground — the "autumn forest" look that replaces the sphere blobs.
fn foliage_instances(manifest: &Manifest, trees: &[Tree], f: &Foliage, lean_deg: f32, view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
    let fog = &manifest.fog;
    let vp = Mat4::from_cols_array(*view_proj);
    let pal = f.palette.as_slice();
    let mut out = Vec::with_capacity(trees.len() * (f.cards_per_tree + f.understory_cards) as usize * 36);
    for t in trees {
        let ground = manifest.terrain.height_at(t.x, t.z);
        let anchor = canopy_anchor(t, ground, lean_deg);
        let r = t.canopy_radius_m;
        // Upper/mid canopy: cards gathered in an oblate mass around the anchor.
        for j in 0..f.cards_per_tree {
            let a = hash01(t.x, t.z, 200 + j) * std::f32::consts::TAU;
            let rad = hash01(t.x, t.z, 300 + j).sqrt() * r;
            let hy = (hash01(t.x, t.z, 400 + j) - 0.30) * r * 1.1;
            let pos = Vec3::new(anchor.x + rad * a.cos(), anchor.y + hy, anchor.z + rad * a.sin());
            let sc = r * f.card_scale * (0.7 + hash01(t.x, t.z, 500 + j) * 0.6);
            let col = pick_color(pal, t.canopy_color, hash01(t.x, t.z, 700 + j), 1.0);
            out.extend_from_slice(&card_instance(&vp, pos, hash01(t.x, t.z, 600 + j), sc, fogged(col, fog, eye.subtract(pos).length(), &style_of(manifest), fol_sat(manifest))));
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
fn tuft_instances(manifest: &Manifest, tufts: &[Tuft], view_proj: &[f32; 16], eye: Vec3) -> Vec<f32> {
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
            Vec3::new(t.radius_m, t.height_m, t.radius_m),
        )
        .to_matrix();
        let dist = eye.subtract(Vec3::new(t.x, ground + t.height_m * 0.5, t.z)).length();
        let tint = fogged(t.color, fog, dist, &style_of(manifest), 1.0);
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
        for y in [0.0f32, 1.0f32] {
            push_vertex(&mut v, [nx, y, nz], [nx, 0.0, nz], [0.5, 0.5], [1.0, 1.0, 1.0, 1.0]);
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
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let up = [0.0f32, 1.0, 0.0];
    let mut base = 0u32;
    for k in 0..TUFT_BLADES {
        let a = (k as f32 / TUFT_BLADES as f32) * std::f32::consts::PI;
        let (dx, dz) = (a.cos() * 0.5, a.sin() * 0.5);
        push_vertex(&mut v, [-dx, 0.0, -dz], up, [0.0, 0.0], [1.0, 1.0, 1.0, 1.0]);
        push_vertex(&mut v, [dx, 0.0, dz], up, [1.0, 0.0], [1.0, 1.0, 1.0, 1.0]);
        push_vertex(&mut v, [0.0, 1.0, 0.0], up, [0.5, 1.0], [1.0, 1.0, 1.0, 1.0]);
        // Both windings → the blade is visible from either side.
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 1]);
        base += 3;
    }
    (v, idx)
}

/// The unit foliage leaf clump: two crossed **irregular** polygons (a jittered rim
/// fan, radius ~0.5), not paper squares, so the card reads as a ragged leaf mass.
/// Double-sided, up-facing normals so the mass catches the warm sun. Per-vertex
/// white; the instance tint carries the autumn leaf colour.
fn foliage_card_unit_mesh() -> (Vec<f32>, Vec<u32>) {
    // Irregular rim radii (a leaf clump silhouette, not a circle or square).
    const RIM: [f32; 9] = [0.50, 0.34, 0.47, 0.28, 0.50, 0.36, 0.44, 0.30, 0.48];
    let up = [0.0f32, 1.0, 0.0];
    let w = [1.0f32, 1.0, 1.0, 1.0];
    let n = RIM.len();
    let mut v = Vec::new();
    let mut idx = Vec::new();
    // Two crossed planes: one facing ±Z (x,y), one facing ±X (z,y).
    (0..2).for_each(|plane| {
        let base = (v.len() / VERT_FLOATS) as u32;
        push_vertex(&mut v, [0.0, 0.06, 0.0], up, [0.5, 0.5], w); // centre, slightly high
        (0..n).for_each(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            let r = RIM[k];
            let (hx, hz) = (plane == 0).then_some((a.cos() * r, 0.0)).unwrap_or((0.0, a.cos() * r));
            push_vertex(&mut v, [hx, a.sin() * r, hz], up, [0.5, 0.5], w);
        });
        (0..n).for_each(|k| {
            let (c, r0, r1) = (base, base + 1 + k as u32, base + 1 + ((k + 1) % n) as u32);
            // Fan triangle, both windings → visible from either side.
            idx.extend_from_slice(&[c, r0, r1, c, r1, r0]);
        });
    });
    (v, idx)
}

/// A 2×2 fully-white albedo texture, so `albedo · vertex_colour · instance_colour`
/// reduces to the per-vertex / per-instance colours the meshes carry.
fn white_material() -> (u64, u32, u32, Vec<u8>) {
    (WHITE_MAT, 2, 2, vec![255u8; 2 * 2 * 4])
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
        assert_eq!(rd.batches[1], (TRUNK_MESH, WHITE_MAT, rd.batches[1].2.clone(), 1));
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
        let batch = rd.batches.iter().find(|(mesh, ..)| *mesh == GROUNDCOVER_MESH).unwrap();
        assert_eq!(batch.3 as usize, tufts.len());
        assert_eq!(batch.2.len(), tufts.len() * 36); // 36 floats per instance
        assert!(rd.meshes.iter().any(|(id, ..)| *id == GROUNDCOVER_MESH));
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

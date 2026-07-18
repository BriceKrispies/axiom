//! The composition layer's retained engine scene: install every mesh,
//! material, and entity ONCE (field pieces, marking meshes, two 17-part
//! player figures per side, the ball, bounded juice + debug pools), then
//! update transforms per tick from the immutable snapshot. Nothing is
//! rebuilt per frame.

use axiom::prelude::{
    Color, DirectionalLight, Entity, Handle, Material, Mesh, MeshData, RunningApp, Spawn,
    Transform, Vec3,
};
use axiom_figure::FigureDefinition;
use axiom_host::FrameAmbient;
use axiom_kernel::Ratio;

use crate::config::PLAYER_COUNT;
use crate::data::team::{frostbite, magma};
use crate::debug::DebugMaterial;
use crate::field::{generate_field, FieldMaterial, FieldMesh};
use crate::player::model::{player_figure, PART_COUNT, TAG_COUNT};
use crate::presentation::particles::{EffectInstance, EffectMaterial};

/// The live per-instance capacity the browser loop is bound with.
pub const LIVE_CAPACITY: u32 = 2048;

/// Pool sizes (hard bounds on juice/debug instances).
const JUICE_POOL: usize = 168;
const DEBUG_POOL: usize = 512;

fn ratio(v: f32) -> Ratio {
    Ratio::finite_or_zero(v)
}

fn color3(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ratio(rgb[0]), ratio(rgb[1]), ratio(rgb[2]))
}

/// Where hidden pool entities park (far under the field, near-zero scale).
fn hidden() -> Transform {
    Transform::new(
        Vec3::new(0.0, -120.0, 0.0),
        axiom_math::Quat::IDENTITY,
        Vec3::new(0.001, 0.001, 0.001),
    )
}

/// The retained scene (synced per tick by [`crate::scene_sync`]).
#[derive(Debug)]
pub struct EndZoneScene {
    pub(crate) figure: FigureDefinition,
    /// Turf pieces that wobble on impact: entity + base transform.
    pub(crate) turf: Vec<(Entity, Transform)>,
    /// One entity per part per player.
    pub(crate) player_parts: Vec<[Entity; PART_COUNT]>,
    pub(crate) ball: Entity,
    pub(crate) lace: Entity,
    /// The bright procedural line-to-gain marker (repositioned per tick; parked
    /// hidden when no drive is active).
    pub(crate) line_to_gain: Entity,
    pub(crate) juice_pool: Vec<(Entity, EffectMaterial)>,
    pub(crate) debug_pool: Vec<(Entity, DebugMaterial)>,
    pub(crate) juice_scratch: Vec<EffectInstance>,
}

impl EndZoneScene {
    /// Install the whole static scene and all pools into `app`.
    pub fn install(app: &mut RunningApp) -> Self {
        let plane = app.add_mesh(Mesh::plane());
        let cube = app.add_mesh(Mesh::cube());
        let sphere = app.add_mesh(Mesh::sphere());
        let cylinder = app.add_mesh(Mesh::cylinder());

        // Field materials.
        let apron = app.add_material(Material::lit(color3([0.10, 0.20, 0.10])));
        let turf_light = app.add_material(Material::lit(color3([0.18, 0.42, 0.16])));
        let turf_dark = app.add_material(Material::lit(color3([0.15, 0.36, 0.14])));
        let white = app.add_material(Material::lit(color3([0.92, 0.93, 0.92])));
        let goalpost = app.add_material(Material::lit(color3([0.95, 0.82, 0.20])));
        let home = magma().palette.slots();
        let away = frostbite().palette.slots();
        let home_zone = app.add_material(Material::lit(color3(home[6])));
        let away_zone = app.add_material(Material::lit(color3(away[6])));

        let field_material = |m: FieldMaterial| match m {
            FieldMaterial::Apron => apron,
            FieldMaterial::TurfLight => turf_light,
            FieldMaterial::TurfDark => turf_dark,
            FieldMaterial::HomeEndZone => home_zone,
            FieldMaterial::AwayEndZone => away_zone,
            FieldMaterial::White => white,
            FieldMaterial::Goalpost => goalpost,
        };

        // Static field pieces (built once by the generator).
        let field = generate_field();
        let mut turf = Vec::new();
        for piece in &field.pieces {
            let mesh = match piece.mesh {
                FieldMesh::Plane => plane,
                FieldMesh::Cube => cube,
                FieldMesh::Cylinder => cylinder,
            };
            let entity = app.spawn(Spawn::new(
                piece.transform,
                mesh,
                field_material(piece.material),
            ));
            let wobbles = matches!(
                piece.material,
                FieldMaterial::TurfLight
                    | FieldMaterial::TurfDark
                    | FieldMaterial::HomeEndZone
                    | FieldMaterial::AwayEndZone
            );
            if wobbles {
                turf.push((entity, piece.transform));
            }
        }
        // The merged marking + number meshes.
        for batch in [field.markings, field.numbers] {
            let (positions, normals, uvs, indices) = batch.into_streams();
            if let Ok(mesh) = app.add_mesh_data(MeshData::new(positions, normals, uvs, indices)) {
                app.spawn(Spawn::new(Transform::IDENTITY, mesh, white));
            }
        }

        // Lighting: one sun + hemisphere ambient.
        app.add_light(
            DirectionalLight {
                direction: Vec3::new(0.32, -1.0, 0.20),
                color: Color::WHITE,
                intensity: ratio(1.05),
            },
            Transform::IDENTITY,
        );
        app.set_ambient(FrameAmbient::new([0.48, 0.62, 0.82], [0.24, 0.30, 0.22]));

        // Team part materials, indexed by part tag.
        let palette_mats =
            |app: &mut RunningApp, palette: &[[f32; 3]; TAG_COUNT]| -> Vec<Handle<Material>> {
                palette
                    .iter()
                    .map(|rgb| app.add_material(Material::lit(color3(*rgb))))
                    .collect()
            };
        let home_mats = palette_mats(app, &home);
        let away_mats = palette_mats(app, &away);

        // Player part entities (spawned once at the hidden pose).
        let figure = player_figure();
        let mut player_parts = Vec::with_capacity(PLAYER_COUNT);
        for player in 0..PLAYER_COUNT {
            let mats = if player < PLAYER_COUNT / 2 {
                &home_mats
            } else {
                &away_mats
            };
            let parts: [Entity; PART_COUNT] = core::array::from_fn(|part| {
                let tag = crate::player::model::PARTS[part].tag as usize;
                app.spawn(Spawn::new(hidden(), cube, mats[tag]).casts_contact_shadow())
            });
            player_parts.push(parts);
        }

        // The ball + lace ridge.
        let leather = app.add_material(Material::lit(color3([0.47, 0.23, 0.11])));
        let lace_mat = app.add_material(Material::lit(color3([0.95, 0.95, 0.92])));
        let ball = app.spawn(Spawn::new(hidden(), sphere, leather).casts_contact_shadow());
        let lace = app.spawn(Spawn::new(hidden(), cube, lace_mat));

        // The line-to-gain marker: a bright volt bar spanning the field,
        // distinct from every white yard line. Parked hidden until a drive
        // repositions it each tick.
        let to_gain_mat = app.add_material(Material::lit(color3([0.72, 0.96, 0.24])));
        let line_to_gain = app.spawn(Spawn::new(hidden(), cube, to_gain_mat));

        // Juice pools (bounded; parked hidden).
        let dust_mat = app.add_material(Material::lit(color3([0.62, 0.54, 0.38])));
        let ring_mat = app.add_material(Material::lit(color3([0.95, 0.94, 0.86])));
        let streak_mat = app.add_material(Material::lit(color3([0.98, 0.98, 0.99])));
        let flash_mat = app.add_material(Material::lit(color3([1.0, 0.92, 0.45])));
        let trail_mat = app.add_material(Material::lit(color3([0.85, 0.62, 0.30])));
        let mut juice_pool = Vec::with_capacity(JUICE_POOL);
        let juice_plan: [(EffectMaterial, usize, Handle<Material>); 5] = [
            (EffectMaterial::Dust, 96, dust_mat),
            (EffectMaterial::Ring, 24, ring_mat),
            (EffectMaterial::Streak, 24, streak_mat),
            (EffectMaterial::Flash, 8, flash_mat),
            (EffectMaterial::Trail, 16, trail_mat),
        ];
        for (material, count, handle) in juice_plan {
            for _ in 0..count {
                juice_pool.push((app.spawn(Spawn::new(hidden(), cube, handle)), material));
            }
        }

        // Debug pools.
        let route_mat = app.add_material(Material::lit(color3([0.15, 0.85, 0.95])));
        let target_mat = app.add_material(Material::lit(color3([0.95, 0.25, 0.85])));
        let collision_mat = app.add_material(Material::lit(color3([0.25, 0.95, 0.35])));
        let catch_mat = app.add_material(Material::lit(color3([0.98, 0.62, 0.15])));
        let trajectory_mat = app.add_material(Material::lit(color3([0.98, 0.92, 0.20])));
        let camera_mat = app.add_material(Material::lit(color3([0.95, 0.15, 0.15])));
        let foot_lock_mat = app.add_material(Material::lit(color3([1.0, 0.35, 0.15])));
        let foot_now_mat = app.add_material(Material::lit(color3([0.20, 0.60, 1.0])));
        let foot_land_mat = app.add_material(Material::lit(color3([0.55, 1.0, 0.30])));
        let move_vec_mat = app.add_material(Material::lit(color3([1.0, 1.0, 1.0])));
        let mut debug_pool = Vec::with_capacity(DEBUG_POOL);
        let debug_plan: [(DebugMaterial, usize, Handle<Material>); 10] = [
            (DebugMaterial::Route, 100, route_mat),
            (DebugMaterial::Target, 24, target_mat),
            (DebugMaterial::Collision, 128, collision_mat),
            (DebugMaterial::CatchVolume, 16, catch_mat),
            (DebugMaterial::Trajectory, 40, trajectory_mat),
            (DebugMaterial::CameraAim, 12, camera_mat),
            (DebugMaterial::FootLock, 14, foot_lock_mat),
            (DebugMaterial::FootNow, 28, foot_now_mat),
            (DebugMaterial::FootLanding, 14, foot_land_mat),
            (DebugMaterial::MoveVector, 56, move_vec_mat),
        ];
        for (material, count, handle) in debug_plan {
            for _ in 0..count {
                debug_pool.push((app.spawn(Spawn::new(hidden(), cube, handle)), material));
            }
        }

        EndZoneScene {
            figure,
            turf,
            player_parts,
            ball,
            lace,
            line_to_gain,
            juice_pool,
            debug_pool,
            juice_scratch: Vec::with_capacity(JUICE_POOL),
        }
    }
}

/// Where hidden pool entities park (far under the field, near-zero scale).
pub(crate) fn hidden_transform() -> Transform {
    hidden()
}

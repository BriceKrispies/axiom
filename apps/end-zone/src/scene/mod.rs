//! The composition layer's retained engine scene: install every mesh,
//! material, and entity ONCE (static field pieces, two 17-part player figures
//! per side, the ball, and the bounded field-paint / juice / debug pools), then
//! update transforms per tick from the immutable snapshot. Nothing is rebuilt
//! per frame — the camera-driven field paint changes which pool slots are
//! *aimed where*, never how many entities or meshes exist.

use axiom::prelude::{
    Color, DirectionalLight, Entity, Handle, Material, Mesh, RunningApp, Spawn, Transform, Vec3,
};
use axiom_figure::FigureDefinition;
use axiom_host::{FrameAmbient, FramePostProcess};
use axiom_kernel::Ratio;

use crate::config::PLAYER_COUNT;
use crate::data::team::{frostbite, magma};
use crate::debug::DebugMaterial;
use crate::field::{
    generate_field, paint_pool_capacity, FieldMaterial, FieldMesh, PaintQuad, PALETTE,
};
use crate::player::model::{player_figure, PART_COUNT, TAG_COUNT};
use crate::presentation::chalk::ChalkMaterial;
use crate::presentation::particles::{EffectInstance, EffectMaterial};
use crate::presentation::receiver_ring::RingKind;

mod pools;

/// The live per-instance capacity the browser loop is bound with.
pub const LIVE_CAPACITY: u32 = 2048;

/// Pool size (hard bound on juice instances; the debug bound lives in `pools`).
const JUICE_POOL: usize = 168;

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
    /// The field paint slots, grouped by [`crate::field::PaintCategory`] (see
    /// [`pools::paint`]). Re-aimed every tick from the camera.
    pub(crate) paint_pool: Vec<Entity>,
    /// Reusable buffer for the paint the camera selects — sized once at install
    /// so the per-frame path never allocates.
    pub(crate) paint_scratch: Vec<PaintQuad>,
    /// White rings at the feet of every receiver the quarterback can throw to.
    pub(crate) receiver_ring_pool: Vec<(Entity, RingKind)>,
    /// Pre-snap route chalk dots (the called play drawn on the turf).
    pub(crate) chalk_pool: Vec<(Entity, ChalkMaterial)>,
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

        // Field materials. The three surface values come from the one field
        // palette (`field::PALETTE`), so the turf the bands alternate between
        // and the paint drawn on top are graded against each other in a single
        // place rather than drifting apart in two.
        //
        // The two band values are deliberately CLOSE and dark: the bands exist
        // to give the eye a distance ruler, and a wide light/dark delta turns
        // that ruler into stripes that fight the players for attention. The
        // separation that used to live in the turf now lives in the paint,
        // where it carries actual information.
        let apron = app.add_material(Material::lit(color3(PALETTE.apron)));
        let turf_light = app.add_material(Material::lit(color3(PALETTE.band_light)));
        let turf_dark = app.add_material(Material::lit(color3(PALETTE.band_dark)));
        let goalpost = app.add_material(Material::lit(color3([0.95, 0.82, 0.20])));
        // The bowl in the reference is PACKED with fans, not bare concrete: its
        // dominant surface is a busy team-color speckle, not gray structure. A
        // flat Lambert band can't speckle, but the honest per-band proxy is a
        // mid-value CROWD average, not concrete. The two alternating tier bands
        // carry different warm/cool crowd tones (home reds/skin vs away blues +
        // shaded upper rows), so the bowl reads as rows of seated fans stepping
        // up rather than a slab of gray — matching the reference's filled stands.
        let stands = app.add_material(Material::lit(color3([0.34, 0.24, 0.22])));
        let crowd = app.add_material(Material::lit(color3([0.24, 0.26, 0.33])));
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
            FieldMaterial::Goalpost => goalpost,
            FieldMaterial::Stands => stands,
            FieldMaterial::Crowd => crowd,
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
        // Lighting: one sun (the key carries form + ground contact shadow) plus
        // a low hemisphere fill kept well below the key, so the shaded box sides
        // deepen instead of flooding flat.
        app.add_light(
            DirectionalLight {
                direction: Vec3::new(0.32, -1.0, 0.20),
                color: Color::WHITE,
                intensity: ratio(1.66),
            },
            Transform::IDENTITY,
        );
        app.set_ambient(FrameAmbient::new([0.21, 0.28, 0.39], [0.10, 0.13, 0.10]));
        // Author the frame's filmic grade (the engine's `cinematic()` preset) so
        // the capture and the live arm grade identically, not a flat raster.
        app.set_postprocess(FramePostProcess::cinematic());

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

        // Bounded instance pools (each parked hidden; built once). Their
        // material/count plans live in `pools` so this install stays readable.
        let paint_pool = pools::paint(app, plane);
        let chalk_pool = pools::chalk(app, cube);
        let juice_pool = pools::juice(app, cube);
        let receiver_ring_pool = pools::receiver_rings(app, cube);
        let debug_pool = pools::debug(app, cube);

        EndZoneScene {
            figure,
            receiver_ring_pool,
            chalk_pool,
            turf,
            player_parts,
            ball,
            lace,
            paint_pool,
            paint_scratch: Vec::with_capacity(paint_pool_capacity()),
            juice_pool,
            debug_pool,
            juice_scratch: Vec::with_capacity(JUICE_POOL),
        }
    }
}

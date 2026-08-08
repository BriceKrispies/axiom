//! The retained engine scene: install every mesh, material and entity ONCE, then
//! move transforms per tick.
//!
//! Nothing here is rebuilt per frame. The pitch, the goal frame, the net
//! strands, the two figures, the ball and the bounded pools (the trajectory
//! preview, the debug markers) are all spawned at install and then only ever
//! *posed* — which is what lets the trajectory preview follow a finger at 60 Hz
//! without allocating.

use axiom::prelude::{
    App, Color, DefaultPlugins, DirectionalLight, Entity, FrameAmbient, FramePostProcess, Handle,
    Material, Mesh, Ratio, RunningApp, Spawn, Transform, Vec3, Visible, Window,
};
use axiom_figure::FigureDefinition;

use crate::figure::{soccer_figure, PART_COUNT, PARTS, TAG_COUNT};
use crate::pitch::{generate_pitch, net_strands, NetStrand, PitchMaterial, PitchMesh};

mod palette;
pub mod sync;

pub use palette::{keeper_kit, kicker_kit, Kit};

/// The live per-instance capacity the browser loop is bound with.
pub const LIVE_CAPACITY: u32 = 4096;

/// How many segments the world-space trajectory preview is drawn with.
pub const PREVIEW_SEGMENTS: usize = 34;
/// How many debug markers the scene can show.
pub const DEBUG_SLOTS: usize = 96;

fn ratio(v: f32) -> Ratio {
    Ratio::finite_or_zero(v)
}

pub(crate) fn color3(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ratio(rgb[0]), ratio(rgb[1]), ratio(rgb[2]))
}

/// Where a retired pool slot parks.
pub(crate) fn hidden() -> Transform {
    Transform::new(
        Vec3::new(0.0, -400.0, 0.0),
        axiom_math::Quat::IDENTITY,
        Vec3::new(0.001, 0.001, 0.001),
    )
}

/// The retained scene.
#[derive(Debug)]
pub struct BendItScene {
    pub(crate) figure: FigureDefinition,
    /// One entity per part, for the kicker and then the keeper.
    pub(crate) kicker_parts: [Entity; PART_COUNT],
    pub(crate) keeper_parts: [Entity; PART_COUNT],
    pub(crate) ball: Entity,
    /// A dark panel band on the ball, so its spin is visible.
    pub(crate) ball_panel: Entity,
    /// The net, as strands plus the rest transform each one returns to.
    pub(crate) net: Vec<(Entity, NetStrand)>,
    /// The world-space trajectory preview.
    pub(crate) preview: Vec<Entity>,
    /// The marker on the authored finishing point.
    pub(crate) target_marker: Entity,
    /// Debug markers (`Trajectory`-yellow, keeper-red).
    pub(crate) debug: Vec<Entity>,
    pub(crate) debug_alt: Vec<Entity>,
}

impl BendItScene {
    /// Build the engine app and install the whole scene into it.
    pub fn install(width: u32, height: u32) -> (RunningApp, BendItScene) {
        // A bright daylight sky: it is also the renderer's distance-fog target,
        // so it must never be dark.
        let sky = color3([0.32, 0.58, 0.86]);
        let mut app = App::new()
            .window(
                Window::new(width.max(1), height.max(1))
                    .with_surface_id(crate::CANVAS_ID)
                    .with_clear_color(sky),
            )
            .add_plugins(DefaultPlugins)
            .setup(|_world, _meshes, _materials| {})
            .build();

        let plane = app.add_mesh(Mesh::plane());
        let cube = app.add_mesh(Mesh::cube());
        let sphere = app.add_mesh(Mesh::sphere());
        let cylinder = app.add_mesh(Mesh::cylinder());

        // The pitch. Two mown greens close enough in value to read as distance
        // rather than as stripes, white paint, and a closed horizon behind the
        // goal so the frame reads against a wall instead of against sky.
        let apron = app.add_material(Material::lit(color3([0.16, 0.30, 0.16])));
        let turf_light = app.add_material(Material::lit(color3([0.26, 0.49, 0.24])));
        let turf_dark = app.add_material(Material::lit(color3([0.22, 0.43, 0.21])));
        let paint = app.add_material(Material::lit(color3([0.92, 0.95, 0.92])));
        let frame = app.add_material(Material::lit(color3([0.95, 0.96, 0.97])));
        let hoarding = app.add_material(Material::lit(color3([0.09, 0.12, 0.19])));
        let stand = app.add_material(Material::lit(color3([0.38, 0.40, 0.46])));
        let crowd = app.add_material(Material::lit(color3([0.47, 0.42, 0.45])));
        let pitch_material = |m: PitchMaterial| match m {
            PitchMaterial::Apron => apron,
            PitchMaterial::TurfLight => turf_light,
            PitchMaterial::TurfDark => turf_dark,
            PitchMaterial::Paint => paint,
            PitchMaterial::Frame => frame,
            PitchMaterial::Hoarding => hoarding,
            PitchMaterial::Stand => stand,
            PitchMaterial::Crowd => crowd,
        };
        generate_pitch().iter().for_each(|piece| {
            let mesh = match piece.mesh {
                PitchMesh::Plane => plane,
                PitchMesh::Cube => cube,
                PitchMesh::Cylinder => cylinder,
            };
            app.spawn(Spawn::new(
                piece.transform,
                mesh,
                pitch_material(piece.material),
            ));
        });

        // The net: one strand entity per strand, parked at its rest transform.
        let netting = app.add_material(Material::lit(color3([0.86, 0.89, 0.90])));
        let net: Vec<(Entity, NetStrand)> = net_strands()
            .into_iter()
            .map(|strand| {
                let entity = app.spawn(Spawn::new(
                    sync::strand_transform(&strand, 0.0),
                    cube,
                    netting,
                ));
                (entity, strand)
            })
            .collect();

        // Lighting: one sun carrying form and the ground contact shadow, and a
        // hemisphere fill kept well below it so the box sides deepen rather than
        // flooding flat.
        app.add_light(
            DirectionalLight {
                // The sun comes from over the player's shoulder, travelling toward
                // the goal. Angling it the other way put every camera-facing
                // surface — the keeper, the stands, the front of the goal frame —
                // into its own shadow, which is why the far end read as a dark
                // wall rather than as a lit stadium.
                direction: Vec3::new(0.32, -0.92, -0.34),
                color: Color::WHITE,
                intensity: ratio(1.58),
            },
            Transform::IDENTITY,
        );
        app.set_ambient(FrameAmbient::new([0.30, 0.38, 0.50], [0.14, 0.18, 0.12]));
        app.set_postprocess(FramePostProcess::cinematic());

        // The two figures, under two kits.
        let figure = soccer_figure();
        let kicker_parts = spawn_figure(&mut app, cube, &kicker_kit());
        let keeper_parts = spawn_figure(&mut app, cube, &keeper_kit());

        // The ball, plus a dark panel band that makes its spin legible.
        let leather = app.add_material(Material::lit(color3([0.95, 0.96, 0.97])));
        let panel = app.add_material(Material::lit(color3([0.10, 0.12, 0.16])));
        let ball = app.spawn(Spawn::new(hidden(), sphere, leather).casts_contact_shadow());
        // A flattened SPHERE, not a box: a cube band wide enough to be seen at the
        // equator is also wide enough to cap the ball from a camera looking down
        // at it, which hid the ball inside its own marking.
        let ball_panel = app.spawn(Spawn::new(hidden(), sphere, panel));

        // The world preview of the authored path, and the marker on its finish.
        let preview_mat = app.add_material(Material::lit(color3([0.99, 0.86, 0.22])));
        let preview = (0..PREVIEW_SEGMENTS)
            .map(|_| slot(&mut app, cube, preview_mat))
            .collect();
        let target_mat = app.add_material(Material::lit(color3([1.0, 0.42, 0.24])));
        let target_marker = slot(&mut app, cube, target_mat);

        // Debug markers: the sampled path in one colour, the keeper's read in
        // another. Retired unless the debug view is on.
        let debug_mat = app.add_material(Material::lit(color3([0.25, 0.95, 0.98])));
        let debug_alt_mat = app.add_material(Material::lit(color3([0.98, 0.22, 0.45])));
        let debug = (0..DEBUG_SLOTS).map(|_| slot(&mut app, cube, debug_mat)).collect();
        let debug_alt = (0..DEBUG_SLOTS / 2)
            .map(|_| slot(&mut app, cube, debug_alt_mat))
            .collect();

        let scene = BendItScene {
            figure,
            kicker_parts,
            keeper_parts,
            ball,
            ball_panel,
            net,
            preview,
            target_marker,
            debug,
            debug_alt,
        };
        (app, scene)
    }
}

/// Spawn one retired pool slot: parked *and* invisible, so it costs the renderer
/// nothing until it is claimed.
fn slot(app: &mut RunningApp, mesh: Handle<Mesh>, material: Handle<Material>) -> Entity {
    let entity = app.spawn(Spawn::new(hidden(), mesh, material));
    app.set(entity, Visible(false));
    entity
}

/// Spawn one figure's parts, each in its kit's material for that part's tag.
fn spawn_figure(app: &mut RunningApp, cube: Handle<Mesh>, kit: &Kit) -> [Entity; PART_COUNT] {
    let materials: Vec<Handle<Material>> = (0..TAG_COUNT)
        .map(|tag| app.add_material(Material::lit(color3(kit.slots[tag]))))
        .collect();
    core::array::from_fn(|part| {
        let tag = PARTS[part].tag as usize;
        app.spawn(Spawn::new(hidden(), cube, materials[tag]).casts_contact_shadow())
    })
}

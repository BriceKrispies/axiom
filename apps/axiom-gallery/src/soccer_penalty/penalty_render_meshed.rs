//! The **fidelity** render bridge: author the diorama into the engine scene with
//! real low-poly [`crate::soccer_penalty::penalty_meshes`] geometry instead of
//! catalog cubes, so the actors read as rounded figures rather than a stack of
//! boxes.
//!
//! Why this is separate from [`crate::soccer_penalty::web`]. The live/browser arm
//! authors through the `setup`/`reauthor` closure, whose `Assets<Mesh>` can only
//! name the catalog cube/sphere/plane — it cannot register custom geometry. Real
//! geometry is only reachable through `RunningApp`'s *runtime* authoring
//! (`add_mesh_data` / `add_material` / `spawn` / `set_camera` / `add_light`), so
//! this path builds an empty app and authors the whole diorama post-build. The
//! deterministic headless `tick` (used by the `axiom-shot` screenshot tool and
//! the visual-convergence loop) re-reads the stores each frame and renders it.
//!
//! This is the first phase of the fidelity work: real meshes on the headless
//! path. Unifying the live/browser path (letting the authoring closure accept
//! `MeshData`) is a later, engine-level step; until then the browser still uses
//! [`crate::soccer_penalty::web::author_soccer`]'s box path.

use axiom::prelude::*;

use crate::soccer_penalty::low_poly_assets::PrimitiveShape;
use crate::soccer_penalty::penalty_meshes::{unit_capsule, unit_cube, unit_sphere};
use crate::soccer_penalty::penalty_render_plan::PenaltyRenderContent;
use crate::soccer_penalty::penalty_scene::DioramaRole;
use crate::soccer_penalty::soccer_penalty_app::Stage1Diorama;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;

/// A finite `Ratio` from a colour channel (clamped, so always valid).
fn ch(value: f32) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).expect("clamped colour channel is finite")
}

/// Keep flat quad/line slabs genuinely thin without collapsing to a zero extent
/// (the unit cube has extent 1, so scale = size).
fn nonzero(s: Vec3) -> Vec3 {
    let c = |v: f32| if v.abs() < 1.0e-3 { 0.01 } else { v };
    Vec3::new(c(s.x), c(s.y), c(s.z))
}

/// Which library mesh renders one diorama object, and the scale to apply. Actor
/// (kicker/goalie) body parts become rounded meshes — spheres for heads/hands,
/// capsules for limbs and torso — while structure (posts, wall, crowd, ad boards,
/// ground quads, net lines) stays boxy, as in the reference.
fn select_mesh(
    role: DioramaRole,
    label: &str,
    shape: PrimitiveShape,
    size: Vec3,
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    capsule: Handle<Mesh>,
) -> (Handle<Mesh>, Vec3) {
    let is_actor = matches!(role, DioramaRole::Kicker | DioramaRole::Goalie);
    let is_round_end = label.ends_with(".head") || label.contains("hand");
    let is_limb = label.contains("arm")
        || label.contains("leg")
        || label.contains("thigh")
        || label.contains("shin")
        || label.contains("foot");
    match (shape, is_actor, is_round_end, is_limb) {
        // The ball: a real sphere (its size.x is a radius, so diameter = size*2).
        (PrimitiveShape::FacetedBall, _, _, _) => (sphere, size.mul_scalar(2.0)),
        // Actor heads and hands → spheres; limbs and the rest of the body → capsules.
        (PrimitiveShape::Box, true, true, _) => (sphere, nonzero(size)),
        (PrimitiveShape::Box, true, false, _) => (capsule, nonzero(size)),
        // Everything structural (and flat quads / thin net lines) → boxes.
        _ => (cube, nonzero(size)),
    }
}

/// Build the meshed headless [`RunningApp`] for a frame: register the mesh
/// library once, spawn one real-mesh renderable per world object (with the
/// plan's painter's-order depth bias toward the camera), then set the camera and
/// key light.
pub fn soccer_meshed_app(frame: Stage1Diorama) -> RunningApp {
    let mut app = App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_clear_color(Color::linear_rgb(ch(0.07), ch(0.10), ch(0.18))),
        )
        .add_plugins(DefaultPlugins)
        .build();

    let cube = app.add_mesh_data(unit_cube()).expect("unit cube geometry is valid");
    let sphere = app.add_mesh_data(unit_sphere()).expect("unit sphere geometry is valid");
    let capsule = app.add_mesh_data(unit_capsule()).expect("unit capsule geometry is valid");

    let cam = frame.render_plan.camera;
    let mut index = 0u64;
    frame.render_plan.items.iter().for_each(|item| {
        if let PenaltyRenderContent::World { role, shape, position, size, shaded_color, .. } =
            item.content
        {
            let (mesh, scale) = select_mesh(role, item.label, shape, size, cube, sphere, capsule);
            // Painter's-order depth bias a hair toward the camera, so the many
            // near-coplanar ground/net quads win the depth test in plan order.
            let to_eye = cam.eye.subtract(position);
            let dir = to_eye.mul_scalar(1.0 / to_eye.length().max(1.0e-6));
            let biased = position.add(dir.mul_scalar(index as f32 * 0.0015));
            let material = app.add_material(Material::lit(Color::linear_rgb(
                ch(shaded_color.r),
                ch(shaded_color.g),
                ch(shaded_color.b),
            )));
            app.spawn(Spawn::new(
                Transform::combine(
                    Transform::from_translation(biased),
                    Transform::from_scale(scale),
                ),
                mesh,
                material,
            ));
            index += 1;
        }
    });

    app.set_camera(
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(cam.fov_y_degrees),
            near: Meters::new(cam.near).expect("camera near plane is finite"),
            far: Meters::new(cam.far).expect("camera far plane is finite"),
        }),
        Transform::from_translation(cam.eye)
            .looking_at(cam.target, cam.up)
            .unwrap_or_else(|_| Transform::from_translation(cam.eye)),
    );
    app.add_light(
        DirectionalLight {
            direction: Vec3::new(0.3, -1.0, 0.4),
            color: Color::WHITE,
            intensity: ch(1.0),
        },
        Transform::IDENTITY,
    );
    app
}

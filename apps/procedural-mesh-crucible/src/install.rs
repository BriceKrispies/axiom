//! Installing the generated scene into a running engine app.
//!
//! This is the one place `axiom-mesh` geometry crosses into the engine, and the
//! crossing is deliberately trivial: `axiom::prelude::Vec3` *is*
//! `axiom_math::Vec3`, so a generated mesh becomes a `MeshData` by handing over
//! its four streams — no conversion, no re-layout, no copy of a copy. If that
//! ever stops being true it is a signal that the umbrella and the mesh layer
//! have drifted, and the fix belongs at that boundary rather than here.
//!
//! The same function serves the browser arm and the native harness, so what a
//! native test builds is byte-for-byte what the page presents.

use axiom::prelude::*;

use crate::debug_view::DebugView;
use crate::locomotion::CrucibleAnimation;
use crate::scene::crucible_scene;
use crate::variant::CrucibleVariant;

/// Where the camera sits and what it looks at.
///
/// This pair is the app's *one* authored framing. `src/orbit.rs` derives the
/// interactive camera's opening yaw/pitch/distance from it rather than typing a
/// second copy of the same shot, so moving these numbers moves both.
pub(crate) const CAMERA_EYE: [f32; 3] = [0.0, 30.0, -112.0];
pub(crate) const CAMERA_TARGET: [f32; 3] = [0.0, 3.0, -22.0];

/// The camera's vertical field of view, in degrees. Shared with `src/orbit.rs`,
/// which needs it to make a pan track the pointer at exactly 1:1.
pub(crate) const CAMERA_FOV_DEGREES: f32 = 58.0;

/// An installed crucible: the scene node each generated object was spawned as,
/// and the animation that moves the two creatures through them.
///
/// The entity list is what makes the creatures animatable at all. Geometry is
/// uploaded once, at bind; from then on the only thing a frame may change is an
/// **instance transform**, and an instance transform is addressed by the entity
/// that carries it. Handing them back here — rather than counting objects and
/// hoping the ids line up — is what keeps the animation bound to the bones it
/// was built from.
#[derive(Debug)]
pub struct InstalledCrucible {
    /// One entity per generated object, in `crucible_meshes` order.
    pub entities: Vec<Entity>,
    /// Where the creature bones start in `entities`.
    pub creatures_first: usize,
    /// The dog's and the human's locomotion.
    pub animation: CrucibleAnimation,
}

impl InstalledCrucible {
    /// Re-author every creature bone's instance transform for `tick`.
    ///
    /// This is the whole per-frame animation cost: no geometry, no allocation
    /// beyond the transform vector, no scene rebuild. Everything static in the
    /// scene is left exactly as it was spawned.
    pub fn animate(&self, running: &mut RunningApp, tick: u64) {
        self.animation
            .transforms(tick)
            .into_iter()
            .zip(self.entities.iter().skip(self.creatures_first))
            .for_each(|(placement, entity)| {
                running.set::<Transform>(*entity, placement);
            });
    }
}

/// Install every generated object, a light rig, and the framing camera.
///
/// `chart_texture` is the id of the app-authored normal chart, present only in
/// the view that samples it.
pub fn install_crucible(
    running: &mut RunningApp,
    variant: CrucibleVariant,
    view: DebugView,
    chart_texture: Option<u64>,
) -> InstalledCrucible {
    let scene = crucible_scene(variant).expect("the authored crucible scene is valid geometry");
    let entities = scene
        .objects
        .iter()
        .map(|object| {
            let drawn = view
                .apply(&object.mesh)
                .expect("a debug view re-normals valid geometry into valid geometry");
            let mesh = running
                .add_mesh_data(MeshData::new(
                    drawn.positions().to_vec(),
                    drawn.normals().to_vec(),
                    drawn.uvs().to_vec(),
                    drawn.indices().to_vec(),
                ))
                .expect("generated geometry registers as engine mesh data");
            let material = running.add_material(material_for(object.color, chart_texture));
            running.spawn(Spawn::new(object.placement, mesh, material))
        })
        .collect();
    install_lights(running);
    install_camera(running);
    InstalledCrucible {
        entities,
        creatures_first: scene.dog_first,
        animation: CrucibleAnimation::new(scene.dog, scene.human)
            .expect("the authored perimeter loop is a valid closed path"),
    }
}

/// The material for one object: its authored colour, or the shared normal chart
/// over white when the chart view is up.
fn material_for(color: [f32; 3], chart_texture: Option<u64>) -> Material {
    let base = Color::linear_rgb(chan(color[0]), chan(color[1]), chan(color[2]));
    chart_texture
        .map(|id| Material::lit(Color::WHITE).with_custom_texture(id))
        .unwrap_or_else(|| Material::lit(base))
}

/// A key sun plus two fill lights, so a swept surface's curvature and a lathed
/// wheel's round both read instead of flattening into one tone.
///
/// The point-light intensities are deliberately modest. A point light falls off
/// with the square of distance, and these sit tens of metres from the geometry
/// they are shaping, so the arithmetic *invites* a four-figure intensity — which
/// then saturates every surface within its own radius to flat white and destroys
/// exactly the shading the crucible exists to show. The sun carries the scene;
/// these two only tint its two sides.
fn install_lights(running: &mut RunningApp) {
    running.add_light(
        DirectionalLight {
            direction: Vec3::new(0.38, -1.0, 0.42),
            color: Color::WHITE,
            intensity: chan(0.85),
        },
        Transform::IDENTITY,
    );
    running.add_point_light(
        PointLight {
            color: Color::linear_rgb(chan(1.0), chan(0.86), chan(0.62)),
            intensity: chan(140.0),
        },
        Transform::from_translation(Vec3::new(-34.0, 26.0, -44.0)),
    );
    running.add_point_light(
        PointLight {
            color: Color::linear_rgb(chan(0.55), chan(0.72), chan(1.0)),
            intensity: chan(110.0),
        },
        Transform::from_translation(Vec3::new(40.0, 22.0, 4.0)),
    );
}

/// The crucible's camera intrinsics. One definition, used by the installed
/// framing below and re-applied every frame by the orbit camera — the lens does
/// not change when the user drags, only the transform does.
pub(crate) fn crucible_camera() -> Camera {
    Camera::perspective(PerspectiveProjection {
        fov_y: Angle::degrees(CAMERA_FOV_DEGREES),
        near: Meters::finite_or_zero(0.4),
        far: Meters::finite_or_zero(700.0),
    })
}

/// The framing camera: high, pulled back, looking down the scene so the
/// reference row is in front and the road runs away into the distance.
fn install_camera(running: &mut RunningApp) {
    let eye = Vec3::new(CAMERA_EYE[0], CAMERA_EYE[1], CAMERA_EYE[2]);
    let target = Vec3::new(CAMERA_TARGET[0], CAMERA_TARGET[1], CAMERA_TARGET[2]);
    running.set_camera(
        crucible_camera(),
        Transform::from_translation(eye)
            .looking_at(target, Vec3::UNIT_Y)
            .expect("the authored camera does not look straight up its own axis"),
    );
}

/// A `0..1` intensity (or an unbounded light intensity) as a validated ratio.
fn chan(value: f32) -> Ratio {
    Ratio::finite_or_zero(value)
}

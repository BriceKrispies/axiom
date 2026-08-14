//! Installing the generated scene into a running engine app.
//!
//! This is the one place `axiom-mesh` geometry crosses into the engine, and the
//! crossing is deliberately trivial: `axiom::prelude::Vec3` *is*
//! `axiom_math::Vec3`, so a generated mesh becomes a `MeshData` by handing over
//! its four streams — no conversion, no re-layout, no copy of a copy. If that
//! ever stops being true it is a signal that the umbrella and the mesh layer
//! have drifted, and the fix belongs at that boundary rather than here.
//!
//! ## Geometry is registered once, and instanced
//!
//! The scene holds eight concentric rings of dogs, and every one of them is the
//! **same** 23 bone meshes. Those meshes are uploaded once, here, and each dog is
//! then `bone_count` more *instances* of them — a transform and a material
//! apiece. Registering a mesh per dog would multiply the vertex upload and the
//! GPU memory by the crowd size for no visible difference whatsoever, so this
//! function is written to make that mistake hard: it registers from
//! `scene.objects` (which is the distinct-mesh set, by construction) and spawns
//! from `scene.dogs` (which carries no geometry at all).
//!
//! ## Materials are registered once too, and *shared*
//!
//! The same argument applies a second time, for a less obvious reason. The live
//! backend batches draws on the `(mesh_id, material_id)` pair, and a draw's
//! colour reaches the GPU only through its material — so a material per dog
//! would mean `23 × dogs` single-instance batches (2760 draw calls here), which
//! is instancing thrown away. The palette in `rings.rs` is therefore registered
//! **once**, `PALETTE_SIZE` materials in total, and every dog names one of them:
//! the draw-call count is at most `23 × PALETTE_SIZE + 1 = 415` — 392 for the
//! field as laid out, which wears 17 of the 18 coats — whatever the crowd size.
//!
//! The same function serves the browser arm and the native harness, so what a
//! native test builds is byte-for-byte what the page presents.

use axiom::prelude::*;

use crate::debug_view::DebugView;
use crate::locomotion::CrucibleAnimation;
use crate::rings::palette;
use crate::scene::crucible_scene;
use crate::variant::CrucibleVariant;

/// Where the camera sits and what it looks at.
///
/// This pair is the app's *one* authored framing. `src/orbit.rs` derives the
/// interactive camera's opening yaw/pitch/distance from it rather than typing a
/// second copy of the same shot, so moving these numbers moves both.
///
/// It is set to frame **the whole filled field**: the outermost ring is 82 units
/// from the origin and its dogs bulge to ~85, so the shot has to hold a
/// 170-unit disc of 11-unit-tall animals. Three numbers decide it:
///
/// * **Distance (195 units).** At a 58° vertical field the binding constraint is
///   the *near* rim — the edge of the disc closest to the camera, which
///   perspective magnifies most. Holding it inside the frustum needs at least
///   159 units at this elevation; the rest is margin, so the front rank is not
///   jammed against the bottom of the frame and the whole 192-unit terrain plate
///   comes into shot with it.
/// * **Elevation (37°).** Steeper than the two-ring shot's 30°, because eight
///   nested rings need enough plan view to be read *as* nested — and a steeper
///   look also squares up the disc, which is what stops the near half from
///   sprawling across the bottom of the frame while the far half shrinks to a
///   band. It is still shallow enough that the dogs are seen in profile and the
///   alternating windings read as opposite rather than as one blur.
/// * **Target (the basin floor, `y = −6`).** The terrain scoops a shallow bowl
///   whose middle sits ~8 units below zero, so this is the actual centre of the
///   thing being framed rather than an arbitrary origin — and aiming at it lifts
///   the field off the bottom edge into the middle of the frame.
pub(crate) const CAMERA_EYE: [f32; 3] = [0.0, 112.0, -155.0];
pub(crate) const CAMERA_TARGET: [f32; 3] = [0.0, -6.0, 0.0];

/// The camera's vertical field of view, in degrees. Shared with `src/orbit.rs`,
/// which needs it to make a pan track the pointer at exactly 1:1.
pub(crate) const CAMERA_FOV_DEGREES: f32 = 58.0;

/// An installed crucible: the scene node each spawned instance was created as,
/// and the animation that walks the whole field through them.
///
/// The entity list is what makes the dogs animatable at all. Geometry is
/// uploaded once, at bind; from then on the only thing a frame may change is an
/// **instance transform**, and an instance transform is addressed by the entity
/// that carries it. Handing them back here — rather than counting objects and
/// hoping the ids line up — is what keeps the animation bound to the bones it
/// was built from.
#[derive(Debug)]
pub struct InstalledCrucible {
    /// Every spawned instance: the static objects first, then every dog's bones
    /// in [`CrucibleAnimation::transforms`] order.
    pub entities: Vec<Entity>,
    /// Where the dogs' bones start in `entities`.
    pub creatures_first: usize,
    /// The field's locomotion.
    pub animation: CrucibleAnimation,
}

impl InstalledCrucible {
    /// Re-author every dog bone's instance transform for `tick`.
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

/// Install every generated mesh, every ring of dogs, a light rig and the framing
/// camera.
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
    // One registration per distinct mesh — the terrain, then the dog's bones.
    let meshes: Vec<Handle<Mesh>> = scene
        .objects
        .iter()
        .map(|object| {
            let drawn = view
                .apply(&object.mesh)
                .expect("a debug view re-normals valid geometry into valid geometry");
            running
                .add_mesh_data(MeshData::new(
                    drawn.positions().to_vec(),
                    drawn.normals().to_vec(),
                    drawn.uvs().to_vec(),
                    drawn.indices().to_vec(),
                ))
                .expect("generated geometry registers as engine mesh data")
        })
        .collect();

    // The static half of the scene: one instance each, where it was authored.
    let mut entities: Vec<Entity> = scene.objects[..scene.dog_first]
        .iter()
        .zip(meshes.iter())
        .map(|(object, mesh)| {
            let material = running.add_material(material_for(object.color, chart_texture));
            running.spawn(Spawn::new(object.placement, *mesh, material))
        })
        .collect();
    let creatures_first = entities.len();

    // The shared coats: one material per palette entry, registered once for the
    // whole field. This is the batching contract — see the module note.
    let coats: Vec<Handle<Material>> = palette()
        .into_iter()
        .map(|color| running.add_material(material_for(color, chart_texture)))
        .collect();

    // The crowd: every dog is `bone_count` more instances of the bone meshes
    // already registered above, wearing one of the coats already registered
    // above. Adding a dog costs neither a vertex nor a material.
    let bones = &meshes[scene.dog_first..];
    scene.dogs.iter().for_each(|dog| {
        let material = coats[dog.palette.min(coats.len() - 1)];
        bones.iter().for_each(|mesh| {
            entities.push(running.spawn(Spawn::new(Transform::IDENTITY, *mesh, material)));
        });
    });

    install_lights(running);
    install_camera(running);
    let installed = InstalledCrucible {
        entities,
        creatures_first,
        animation: CrucibleAnimation::new(scene.dog)
            .expect("the authored rings are valid closed paths"),
    };
    // Stand the crowd on its first frame straight away, so a headless build that
    // never calls `animate` still presents the scene the page opens on rather
    // than the whole field collapsed onto the origin.
    installed.animate(running, 0);
    installed
}

/// The material for one instance: its authored colour, or the shared normal
/// chart over white when the chart view is up.
fn material_for(color: [f32; 3], chart_texture: Option<u64>) -> Material {
    let base = Color::linear_rgb(chan(color[0]), chan(color[1]), chan(color[2]));
    chart_texture
        .map(|id| Material::lit(Color::WHITE).with_custom_texture(id))
        .unwrap_or_else(|| Material::lit(base))
}

/// A key sun plus two fill lights, so a swept limb's curvature and a lofted
/// torso's round both read instead of flattening into one tone.
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

/// The framing camera: high and pulled back, looking down at the middle of the
/// field.
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

//! Installing the generated scene into a running engine app, and re-authoring it
//! every frame from the live configuration.
//!
//! This is the one place `axiom-mesh` geometry crosses into the engine, and the
//! crossing is deliberately trivial: `axiom::prelude::Vec3` *is*
//! `axiom_math::Vec3`, so a generated mesh becomes a `MeshData` by handing over
//! its four streams — no conversion, no re-layout, no copy of a copy.
//!
//! ## Geometry is registered once, and instanced
//!
//! Every dog in the field is the **same** 23 bone meshes. Those meshes are
//! uploaded once, here, and each dog is then `bone_count` more *instances* of
//! them — a transform and a material apiece. This function is written to make the
//! opposite mistake hard: it registers from `scene.objects` (the distinct-mesh
//! set, by construction) and spawns from the **pool**, which carries no geometry
//! at all.
//!
//! ## The pool, and why the crowd is spawned at its maximum
//!
//! The ring dials move the crowd size, and the live backend uploads geometry once
//! at bind — so the honest way to add a dog at frame 400 is to have spawned it at
//! frame 0 and left it retired. [`install_scene`] therefore spawns
//! [`MAX_DOGS`] dogs' worth of bone nodes up front and
//! [`InstalledScene::animate`] draws only the ones the current layout asks for,
//! writing `Visible(false)` over the rest.
//!
//! That is the engine's own sanctioned pooling primitive rather than a workaround:
//! an invisible renderable is dropped at submission, so it costs no projection,
//! no shading and no draw — and unlike a despawn it keeps the entity, so the next
//! flick of the ring dial is a visibility write instead of a scene rebuild. The
//! visibility is only re-written when the *count* changes, so a steady scene pays
//! nothing at all.
//!
//! ## Materials are registered once too, and *shared*
//!
//! The live backend batches draws on the `(mesh_id, material_id)` pair, and a
//! draw's colour reaches the GPU only through its material — so a material per
//! dog would mean `23 × dogs` single-instance batches, which is instancing thrown
//! away. The palette in `rings.rs` is therefore registered **once**,
//! `PALETTE_SIZE` materials in total, and every dog names one of them: the
//! draw-call count is at most `23 × PALETTE_SIZE + 1 = 415` whatever the crowd
//! size.
//!
//! That same fact is why a **pool slot's coat is fixed at spawn**: `Material` has
//! no runtime mutation and `Renderable` is not a settable component, so an
//! installed instance cannot be repainted. The pool therefore hands slot `n` the
//! coat `n % PALETTE_SIZE` for good, and the layout's palette assignment is
//! honoured by *which slot* each ring-dog is drawn in. See `NOTES.md` for why
//! there is no colour dial.
//!
//! The same function serves the browser arm and the native harness, so what a
//! native test builds is byte-for-byte what the page presents.

use axiom::prelude::*;

use crate::config::SceneConfig;
use crate::debug_view::DebugView;
use crate::locomotion::Animation;
use crate::rings::{palette, MAX_DOGS, PALETTE_SIZE};
use crate::scene::build_scene;
use crate::variant::SceneVariant;

/// Where the camera sits and what it looks at.
///
/// This pair is the app's *one* authored framing. `src/orbit.rs` derives the
/// interactive camera's opening yaw/pitch/distance from it rather than typing a
/// second copy of the same shot, so moving these numbers moves both.
///
/// It is set to frame **the whole filled field** at the opening configuration:
/// the outermost ring is 80.25 units from the origin and its dogs bulge to ~83,
/// so the shot has to hold a 166-unit disc of 8-unit-tall animals. Three numbers
/// decide it:
///
/// * **Distance (195 units).** At a 58° vertical field the binding constraint is
///   the *near* rim — the edge of the disc closest to the camera, which
///   perspective magnifies most. Holding it inside the frustum needs at least
///   156 units at this elevation; the rest is margin, so the front rank is not
///   jammed against the bottom of the frame and the whole 192-unit terrain plate
///   comes into shot with it.
/// * **Elevation (37°).** Steeper than a two-ring shot's 30°, because eight
///   nested rings need enough plan view to be read *as* nested — and a steeper
///   look also squares up the disc, which is what stops the near half from
///   sprawling across the bottom of the frame while the far half shrinks to a
///   band. It is still shallow enough that the dogs are seen in profile.
/// * **Target (the basin floor, `y = −6`).** The terrain scoops a shallow bowl
///   whose middle sits ~8 units below zero, so this is the actual centre of the
///   thing being framed rather than an arbitrary origin — and aiming at it lifts
///   the field off the bottom edge into the middle of the frame.
pub(crate) const CAMERA_EYE: [f32; 3] = [0.0, 112.0, -155.0];
pub(crate) const CAMERA_TARGET: [f32; 3] = [0.0, -6.0, 0.0];

/// The camera's vertical field of view, in degrees. Shared with `src/orbit.rs`,
/// which needs it to make a pan track the pointer at exactly 1:1.
pub(crate) const CAMERA_FOV_DEGREES: f32 = 58.0;

/// An installed scene: the pool of nodes every dog is drawn in, and the
/// animation that walks the field through them.
///
/// The entity list is what makes the dogs animatable at all. Geometry is
/// uploaded once, at bind; from then on the only thing a frame may change is an
/// **instance transform** (and a visibility flag), and both are addressed by the
/// entity that carries them. Handing them back here — rather than counting
/// objects and hoping the ids line up — is what keeps the animation bound to the
/// bones it was built from.
#[derive(Debug)]
pub struct InstalledScene {
    /// Every spawned instance: the static objects first, then [`MAX_DOGS`] dogs'
    /// bones in [`Animation::transforms`] order.
    pub entities: Vec<Entity>,
    /// Where the dogs' bones start in `entities`.
    pub creatures_first: usize,
    /// How many bones one dog has.
    pub bone_count: usize,
    /// The field's locomotion, at the configuration it was last built for.
    pub animation: Animation,
    /// The configuration `animation` and the pool's visibility currently reflect.
    applied: SceneConfig,
    /// How many pool slots are currently visible.
    shown: usize,
}

impl InstalledScene {
    /// Re-author the scene for `tick` at `config`.
    ///
    /// Three tiers of work, and which tier runs is decided by what actually
    /// moved:
    ///
    /// 1. **Every frame** — one instance transform per visible bone. No geometry,
    ///    no allocation beyond the transform vector, no scene rebuild.
    /// 2. **When a gait or speed dial moves** — the resolved gait is re-read.
    ///    Free: it is a `Copy` struct on the animation.
    /// 3. **When a ring dial moves** — the arc-length tables are re-inverted and
    ///    the pool's visibility is re-written for the delta. This is the only
    ///    expensive path, and it is the one the page's ring sliders drag through.
    pub fn animate(&mut self, running: &mut RunningApp, tick: u64, config: &SceneConfig) {
        self.resettle(running, config);
        self.animation
            .transforms(tick)
            .into_iter()
            .zip(self.entities.iter().skip(self.creatures_first))
            .for_each(|(placement, entity)| {
                running.set::<Transform>(*entity, placement);
            });
    }

    /// Bring the animation and the pool's visibility into line with `config`.
    fn resettle(&mut self, running: &mut RunningApp, config: &SceneConfig) {
        if !self.applied.live_differs(config) {
            return;
        }
        self.applied = *config;
        if self.animation.follows(config) {
            self.animation.retune(config);
            return;
        }
        // A ring dial moved: rebuild the walks, then show exactly the dogs the
        // new layout asks for. A path that will not build (it cannot, at any
        // dial position the config clamps to) leaves the previous one standing
        // rather than emptying the field.
        Animation::new(self.animation.rig().clone(), config)
            .into_iter()
            .for_each(|rebuilt| self.animation = rebuilt);
        self.animation.retune(config);
        let wanted = self.animation.dog_count();
        let (from, to) = (self.shown.min(wanted), self.shown.max(wanted));
        let visible = wanted > self.shown;
        (from..to).for_each(|dog| {
            (0..self.bone_count).for_each(|bone| {
                let slot = self.creatures_first + dog * self.bone_count + bone;
                self.entities
                    .get(slot)
                    .into_iter()
                    .for_each(|entity| {
                        running.set::<Visible>(*entity, Visible(visible));
                    });
            });
        });
        self.shown = wanted;
    }
}

/// Install every generated mesh, the pooled crowd, a light rig and the framing
/// camera.
///
/// `chart_texture` is the id of the app-authored normal chart, present only in
/// the view that samples it.
pub fn install_scene(
    running: &mut RunningApp,
    variant: SceneVariant,
    view: DebugView,
    chart_texture: Option<u64>,
    config: &SceneConfig,
) -> InstalledScene {
    let scene = build_scene(variant, config).expect("the authored scene is valid geometry");
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

    // The pool: MAX_DOGS dogs' worth of bone instances, each permanently wearing
    // the coat its slot names. Adding a dog to the *field* later is then a
    // visibility write, not a spawn.
    let bones = &meshes[scene.dog_first..];
    (0..MAX_DOGS).for_each(|slot| {
        let material = coats[slot % PALETTE_SIZE.min(coats.len().max(1))];
        bones.iter().for_each(|mesh| {
            entities.push(running.spawn(Spawn::new(Transform::IDENTITY, *mesh, material)));
        });
    });

    install_lights(running);
    install_camera(running);
    let mut installed = InstalledScene {
        entities,
        creatures_first,
        bone_count: bones.len(),
        animation: Animation::new(scene.dog, config).expect("the authored rings are valid paths"),
        applied: *config,
        shown: MAX_DOGS,
    };
    // Retire every pool slot the opening layout does not use, then stand the
    // crowd on its first frame — so a headless build that never calls `animate`
    // again still presents the scene the page opens on rather than the whole pool
    // collapsed onto the origin.
    installed.retire_unused(running);
    installed.animate(running, 0, config);
    installed
}

impl InstalledScene {
    /// Hide every pool slot beyond the opening layout's crowd.
    fn retire_unused(&mut self, running: &mut RunningApp) {
        let wanted = self.animation.dog_count();
        (wanted..MAX_DOGS).for_each(|dog| {
            (0..self.bone_count).for_each(|bone| {
                let slot = self.creatures_first + dog * self.bone_count + bone;
                self.entities.get(slot).into_iter().for_each(|entity| {
                    running.set::<Visible>(*entity, Visible(false));
                });
            });
        });
        self.shown = wanted;
    }
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
/// exactly the shading this scene exists to show. The sun carries the scene;
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

/// The scene's camera intrinsics. One definition, used by the installed framing
/// below and re-applied every frame by the orbit camera — the lens does not
/// change when the user drags, only the transform does.
pub(crate) fn scene_camera() -> Camera {
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
        scene_camera(),
        Transform::from_translation(eye)
            .looking_at(target, Vec3::UNIT_Y)
            .expect("the authored camera does not look straight up its own axis"),
    );
}

/// A `0..1` intensity (or an unbounded light intensity) as a validated ratio.
fn chan(value: f32) -> Ratio {
    Ratio::finite_or_zero(value)
}

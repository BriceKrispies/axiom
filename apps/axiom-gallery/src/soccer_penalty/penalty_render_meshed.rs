//! The **single, shared** meshed render bridge: author the diorama into the
//! engine scene with real low-poly [`crate::soccer_penalty::penalty_meshes`]
//! geometry instead of catalog cubes, so the actors read as rounded figures.
//!
//! This is the *one* path both consumers use, so the convergence champion and the
//! live gallery can never diverge:
//!
//! * the **headless champion** ([`soccer_meshed_app`]) — `axiom-shot` / the
//!   visual-convergence loop — builds it once and ticks it, and
//! * the **live browser gallery** ([`crate::soccer_penalty::web::soccer_penalty_start`])
//!   re-authors it every frame.
//!
//! Both go through [`PenaltyMeshedScene`]: the same mesh library, the same
//! shape→mesh mapping, the same per-object spawn.
//!
//! Why not the `setup`/`reauthor` closure. The live/browser path historically
//! authored through that closure, whose `Assets<Mesh>` can only name the catalog
//! cube/sphere/plane — the `Mesh` enum is deliberately fieldless so the engine
//! resolves it branchlessly by table index, so it *cannot* carry custom geometry.
//! Real geometry has its own sanctioned channel: `RunningApp::add_mesh_data`
//! (which stores resolved geometry directly). So this scene registers the mesh
//! library once via that channel and drives per-frame updates with runtime
//! `spawn`/`despawn` — never `reauthor`, which would rebuild the mesh store from
//! the catalog-only closure and wipe the custom meshes.

use std::collections::HashMap;

use axiom::prelude::*;

use crate::soccer_penalty::low_poly_assets::{PrimitiveShape, Rgba};
use crate::soccer_penalty::penalty_meshes::{unit_capsule, unit_cube, unit_sphere};
use crate::soccer_penalty::penalty_render_plan::PenaltyRenderContent;
use crate::soccer_penalty::penalty_scene::DioramaRole;
use crate::soccer_penalty::penalty_textures;
use crate::soccer_penalty::soccer_penalty_app::Stage1Diorama;
use crate::soccer_penalty::static_diorama::CameraConfig;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;

/// A finite `Ratio` from a colour channel (clamped, so always valid).
fn ch(value: f32) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).expect("clamped colour channel is finite")
}

/// Keep flat quad/line slabs thin without collapsing to a zero extent.
fn nonzero(s: Vec3) -> Vec3 {
    let c = |v: f32| if v.abs() < 1.0e-3 { 0.01 } else { v };
    Vec3::new(c(s.x), c(s.y), c(s.z))
}

/// The library mesh + scale for one diorama object. Actor (kicker/goalie) body
/// parts become rounded meshes — spheres for heads/hands, capsules for limbs and
/// torso — while structure (posts, wall, crowd, ad boards, ground quads, net
/// lines) stays boxy, as in the reference.
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
    match (shape, is_actor, is_round_end) {
        (PrimitiveShape::FacetedBall, _, _) => (sphere, size.mul_scalar(2.0)),
        (PrimitiveShape::Box, true, true) => (sphere, nonzero(size)),
        (PrimitiveShape::Box, true, false) => (capsule, nonzero(size)),
        _ => (cube, nonzero(size)),
    }
}

/// The registered low-poly mesh library (one handle per shape family).
#[derive(Clone, Copy, Debug)]
struct MeshLib {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    capsule: Handle<Mesh>,
}

/// The registered retro 32-bit pixel-art texture ids (0 = none), by surface kind.
#[derive(Clone, Copy, Debug)]
struct TexLib {
    crowd: u64,
    ad_axiom: u64,
    ad_generic: u64,
    jersey: u64,
    keeper: u64,
    ball: u64,
    skin: u64,
}

/// The shared meshed scene: the mesh + texture libraries, a
/// (colour, texture)→material cache (so a stable palette of materials is
/// registered before the live backend snapshots them, and no material leaks per
/// frame), and the entities spawned last frame (despawned before the next pass).
#[derive(Debug)]
pub struct PenaltyMeshedScene {
    lib: MeshLib,
    tex: TexLib,
    palette: HashMap<([u8; 3], u64), Handle<Material>>,
    spawned: Vec<Entity>,
}

impl PenaltyMeshedScene {
    /// Register the mesh + texture libraries into `app` and prime an empty scene.
    pub fn install(app: &mut RunningApp) -> Self {
        let lib = MeshLib {
            cube: app.add_mesh_data(unit_cube()).expect("unit cube geometry is valid"),
            sphere: app.add_mesh_data(unit_sphere()).expect("unit sphere geometry is valid"),
            capsule: app.add_mesh_data(unit_capsule()).expect("unit capsule geometry is valid"),
        };
        let mut tex = |t: (u32, u32, Vec<u8>)| {
            app.add_texture_data(t.0, t.1, t.2).expect("authored texture is valid").id()
        };
        let tex = TexLib {
            crowd: tex(penalty_textures::crowd()),
            ad_axiom: tex(penalty_textures::ad_axiom()),
            ad_generic: tex(penalty_textures::ad_generic()),
            jersey: tex(penalty_textures::jersey([40, 76, 200], "10", [240, 240, 245])),
            keeper: tex(penalty_textures::kit([230, 200, 40])),
            ball: tex(penalty_textures::ball()),
            skin: tex(penalty_textures::skin([210, 160, 128])),
        };
        Self { lib, tex, palette: HashMap::new(), spawned: Vec::new() }
    }

    /// The retro 32-bit texture id for one object's role/label (0 = flat, no texture).
    fn texture_for(&self, role: DioramaRole, label: &str) -> u64 {
        match role {
            DioramaRole::CrowdCard => self.tex.crowd,
            DioramaRole::AdBoard => {
                if label == "ad.board.axiom" {
                    self.tex.ad_axiom
                } else {
                    self.tex.ad_generic
                }
            }
            DioramaRole::Kicker => self.body_texture(label, self.tex.jersey),
            DioramaRole::Goalie => self.body_texture(label, self.tex.keeper),
            DioramaRole::Ball => (label == "ball").then_some(self.tex.ball).unwrap_or(0),
            _ => 0,
        }
    }

    /// Body-part texture: the kit on the torso, skin on head/hands, flat elsewhere.
    fn body_texture(&self, label: &str, kit: u64) -> u64 {
        if label.ends_with(".torso") {
            kit
        } else if label.ends_with(".head") || label.contains("hand") {
            self.tex.skin
        } else {
            0
        }
    }

    /// Set the fixed camera and key light once (call after [`Self::install`]).
    pub fn set_view(&self, app: &mut RunningApp, cam: CameraConfig) {
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
    }

    /// A cached lit material for `color` (registered once, reused thereafter), so
    /// the material set the live backend snapshots at startup stays stable.
    fn material_for(&mut self, app: &mut RunningApp, color: Rgba, tex_id: u64) -> Handle<Material> {
        let key = (
            [
                (color.r.clamp(0.0, 1.0) * 255.0) as u8,
                (color.g.clamp(0.0, 1.0) * 255.0) as u8,
                (color.b.clamp(0.0, 1.0) * 255.0) as u8,
            ],
            tex_id,
        );
        *self.palette.entry(key).or_insert_with(|| {
            // The texture (when present) modulates the flat `shaded_color` tint —
            // albedo × base colour — so the retro 32-bit shading survives under the texture.
            let base = Material::lit(Color::linear_rgb(ch(color.r), ch(color.g), ch(color.b)));
            let material = if tex_id != 0 { base.with_custom_texture(tex_id) } else { base };
            app.add_material(material)
        })
    }

    /// Re-author the diorama: despawn the previous frame's renderables, then spawn
    /// this frame's world objects with real meshes (in the plan's back-to-front
    /// order, each nudged a hair toward the camera so near-coplanar ground/net
    /// quads win the depth test). Camera + light are untouched (set once).
    pub fn author(&mut self, app: &mut RunningApp, frame: &Stage1Diorama) {
        self.spawned.drain(..).for_each(|e| {
            app.despawn(e);
        });
        let cam = frame.render_plan.camera;
        let mut index = 0u64;
        // Collect first so the immutable borrow of `frame` doesn't overlap the
        // mutable `self`/`app` borrows in the spawn loop.
        let objects: Vec<_> = frame
            .render_plan
            .items
            .iter()
            .filter_map(|item| match item.content {
                PenaltyRenderContent::World { role, shape, position, size, shaded_color, .. } => {
                    Some((role, item.label, shape, position, size, shaded_color))
                }
                _ => None,
            })
            .collect();
        objects.into_iter().for_each(|(role, label, shape, position, size, color)| {
            let (mesh, scale) =
                select_mesh(role, label, shape, size, self.lib.cube, self.lib.sphere, self.lib.capsule);
            let to_eye = cam.eye.subtract(position);
            let dir = to_eye.mul_scalar(1.0 / to_eye.length().max(1.0e-6));
            let biased = position.add(dir.mul_scalar(index as f32 * 0.0015));
            let material = self.material_for(app, color, self.texture_for(role, label));
            let entity = app.spawn(Spawn::new(
                Transform::combine(
                    Transform::from_translation(biased),
                    Transform::from_scale(scale),
                ),
                mesh,
                material,
            ));
            self.spawned.push(entity);
            index += 1;
        });
    }
}

/// Build the empty meshed app shell (window + clear colour + default plugins).
pub fn soccer_meshed_shell() -> RunningApp {
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_clear_color(Color::linear_rgb(ch(0.07), ch(0.10), ch(0.18))),
        )
        .add_plugins(DefaultPlugins)
        .build()
}

/// Build the headless meshed [`RunningApp`] for one static frame — the
/// convergence champion (`axiom-shot`). Identical authoring to the live gallery.
pub fn soccer_meshed_app(frame: Stage1Diorama) -> RunningApp {
    let mut app = soccer_meshed_shell();
    let mut scene = PenaltyMeshedScene::install(&mut app);
    scene.set_view(&mut app, frame.render_plan.camera);
    scene.author(&mut app, &frame);
    app
}

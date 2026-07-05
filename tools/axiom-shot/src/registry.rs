//! The renderable-slice registry: the single table mapping a slice **name** to a
//! builder that produces its [`RunningApp`].
//!
//! This replaces the old closed 5-arm `match`: `axiom-shot` renders ANY
//! registered slice by name, and adding a slice is adding one row to
//! [`registry`] (plus a `slice.toml`, which `xtask check-slices` cross-checks
//! against this table). The registered names are string literals so the checker
//! can confirm a declared `harness` is present here.
//!
//! Not every gallery demo can be registered: `generia` and `forest_walk` are
//! `#![cfg(target_arch = "wasm32")]` bespoke `run_web_multi` renderers with no
//! native `App`/`RunningApp` core, so they have no native pixel path to capture.

use axiom::prelude::*;
use axiom_animation_lab::scene::LabScene;

/// Authoring / GPU render size (also the window size the scenes request).
pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 600;

/// A linear colour channel / intensity from a known-finite authored literal.
fn ch(v: f32) -> Ratio {
    Ratio::new(v).expect("authored colour channel is finite")
}

/// Per-slice build inputs (each builder reads only what it needs).
#[derive(Debug, Clone, Default)]
pub struct BuildParams {
    /// `--level PATH` for the retro FPS slice (else its built-in default level).
    pub level: Option<String>,
    /// `--shot-tick N` for the soccer slice (a scripted power shot N ticks in).
    pub shot_tick: Option<u32>,
    /// `--frame N` for the animation-lab posed-figure slice.
    pub frame: u32,
    /// `--cubes N` for the stress-cubes slice.
    pub stress_count: u32,
}

/// One registered renderable slice: a stable `name` and a builder for its
/// [`RunningApp`] core.
pub struct SliceEntry {
    pub name: &'static str,
    pub build: fn(&BuildParams) -> RunningApp,
}

impl std::fmt::Debug for SliceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliceEntry").field("name", &self.name).finish()
    }
}

/// The renderable-slice registry — the single source of truth for which slices
/// `axiom-shot` can render by name.
pub fn registry() -> Vec<SliceEntry> {
    vec![
        SliceEntry { name: "showcase", build: |_| showcase_app().build() },
        SliceEntry { name: "nova-roll", build: |_| nova_roll_app().build() },
        SliceEntry {
            name: "rotating-cube",
            build: |_| axiom_gallery::rotating_cube_core(),
        },
        SliceEntry {
            name: "stress-cubes",
            build: |p| axiom_gallery::stress_cubes_core(p.stress_count.max(1)),
        },
        SliceEntry {
            name: "physics-crucible",
            build: |_| axiom_gallery::physics_crucible::build_physics_crucible(),
        },
        SliceEntry { name: "soccer-penalty", build: build_soccer },
        SliceEntry { name: "retro-fps", build: build_retro_fps },
        SliceEntry { name: "animation-lab", build: build_posed_figure },
    ]
}

/// Build the slice registered under `name`, or `None` if unknown.
pub fn build(name: &str, params: &BuildParams) -> Option<RunningApp> {
    registry()
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| (e.build)(params))
}

/// The names of every registered slice (for `--list` and error messages).
pub fn names() -> Vec<&'static str> {
    registry().into_iter().map(|e| e.name).collect()
}

fn build_retro_fps(p: &BuildParams) -> RunningApp {
    axiom_game_retro_fps::build_retro_fps_app(&retro_fps_doc(p.level.as_deref())).0
}

fn build_soccer(p: &BuildParams) -> RunningApp {
    // `--shot-tick N` renders a scripted power shot N ticks in (kick animation +
    // ball flight + keeper dive visible); without it, the static stage-1 diorama.
    let diorama = p
        .shot_tick
        .map(|n| axiom_gallery::soccer_penalty::SoccerPenaltyApp::build_frame(&soccer_shot_state(n)))
        .unwrap_or_else(axiom_gallery::soccer_penalty::SoccerPenaltyApp::build_stage1);
    axiom_gallery::soccer_penalty::penalty_render_meshed::soccer_meshed_app(diorama)
}

/// L3: the animation-lab posed-figure scene captured as REAL pixels (not SVG).
/// Loads the shared kicker figure + kick clip bytes, poses the figure at `frame`,
/// and spawns one box renderable per posed part — the same posed boxes the SVG
/// scrubber draws, now rendered through a real backend.
fn build_posed_figure(p: &BuildParams) -> RunningApp {
    let parts = LabScene::new().view(p.frame).parts;
    App::new()
        .window(Window::new(WIDTH, HEIGHT).with_clear_color(Color::linear_rgb(
            ch(0.06),
            ch(0.07),
            ch(0.09),
        )))
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let body = materials.add(Material::lit(Color::linear_rgb(
                ch(0.82),
                ch(0.78),
                ch(0.70),
            )));
            parts.iter().for_each(|part| {
                // Box_size is the full box extents; the cube mesh spans two units,
                // so scale by half the extents.
                let scale = Vec3::new(
                    part.box_size.x * 0.5,
                    part.box_size.y * 0.5,
                    part.box_size.z * 0.5,
                );
                world.spawn((
                    Transform::combine(part.transform, Transform::from_scale(scale)),
                    Renderable { mesh: cube, material: body },
                ));
            });
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.9, 3.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.5),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
        .build()
}

/// The retro FPS level document for `--level PATH` (else the built-in default).
/// Shared by the registry build and the binary's `--pose` teleport path.
pub fn retro_fps_doc(level: Option<&str>) -> axiom_game_retro_fps::level::LevelDoc {
    match level {
        Some(path) => axiom_game_retro_fps::level::LevelDoc::parse(
            &std::fs::read_to_string(path).expect("read --level file"),
        ),
        None => axiom_game_retro_fps::level::LevelDoc::default(),
    }
}

/// The soccer interaction state `shot_tick` ticks into a standard centred power
/// shot: hold to charge for a few ticks, release, then let the ball fly.
pub fn soccer_shot_state(
    shot_tick: u32,
) -> axiom_gallery::soccer_penalty::PenaltyInteractionState {
    use axiom_gallery::soccer_penalty::{PenaltyInputIntent, PenaltyInteractionState};
    const CHARGE_TICKS: u32 = 8;
    (0..shot_tick).fold(PenaltyInteractionState::start(), |s, t| {
        let intent = if t < CHARGE_TICKS {
            PenaltyInputIntent::charging(0, 0)
        } else if t == CHARGE_TICKS {
            PenaltyInputIntent::releasing()
        } else {
            PenaltyInputIntent::neutral()
        };
        s.advance(intent)
    })
}

/// Author the Stage-2/3 textured + lit showcase: three spinning checker cubes, a
/// UV-grid ground plane, a checker sphere, SDF shapes, a proc-animated cube, a
/// camera, a directional sun, and three orbiting coloured point lights.
pub fn showcase_app() -> App {
    App::new()
        .window(Window::new(WIDTH, HEIGHT).with_clear_color(Color::linear_rgb(
            ch(0.05),
            ch(0.06),
            ch(0.08),
        )))
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let cubes = [
                (-2.6, Vec3::UNIT_Y, Color::linear_rgb(ch(0.85), ch(0.25), ch(0.25))),
                (0.0, Vec3::UNIT_X, Color::linear_rgb(ch(0.30), ch(0.80), ch(0.35))),
                (2.6, Vec3::new(1.0, 1.0, 0.0), Color::linear_rgb(ch(0.30), ch(0.50), ch(0.95))),
            ];
            cubes.into_iter().for_each(|(offset_x, axis, color)| {
                let material = materials.add(Material::lit(color).with_texture(Texture::Checker));
                world
                    .spawn(Transform::from_translation(Vec3::new(offset_x, 0.0, 0.0)))
                    .with_child((
                        Renderable { mesh: cube, material },
                        Spin::around(axis).period(360),
                    ));
            });
            let plane = meshes.add(Mesh::plane());
            let ground = materials.add(
                Material::lit(Color::linear_rgb(ch(0.18), ch(0.20), ch(0.24)))
                    .with_texture(Texture::UvGrid),
            );
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, -2.0, 0.0)),
                    Transform::from_scale(Vec3::new(30.0, 1.0, 30.0)),
                ),
                Renderable { mesh: plane, material: ground },
            ));
            let sphere = meshes.add(Mesh::sphere());
            let sphere_material = materials.add(
                Material::lit(Color::linear_rgb(ch(0.90), ch(0.78), ch(0.30)))
                    .with_texture(Texture::Checker),
            );
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, 2.6, 0.0)),
                    Transform::from_scale(Vec3::new(1.6, 1.6, 1.6)),
                ),
                Renderable { mesh: sphere, material: sphere_material },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(-4.6, 1.6, 0.0)),
                SdfShape::sphere(
                    Meters::new(1.3).expect("finite radius"),
                    Color::linear_rgb(ch(0.20), ch(0.85), ch(0.90)),
                ),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(4.7, 1.4, 0.0)),
                SdfShape::cuboid(
                    Vec3::new(0.9, 0.9, 0.9),
                    Color::linear_rgb(ch(0.95), ch(0.45), ch(0.85)),
                ),
            ));
            let proc_material =
                materials.add(Material::lit(Color::linear_rgb(ch(0.95), ch(0.55), ch(0.12))));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, -0.6, 3.4)),
                Renderable { mesh: cube, material: proc_material },
                ProcAnim::bob(Meters::new(1.6).expect("finite bob"), 120).spin(Vec3::UNIT_Y, 180),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.4),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
            let orbit_lights = [
                (Color::linear_rgb(ch(0.95), ch(0.25), ch(0.25)), 200),
                (Color::linear_rgb(ch(0.25), ch(0.95), ch(0.35)), 320),
                (Color::linear_rgb(ch(0.30), ch(0.45), ch(0.98)), 260),
            ];
            orbit_lights.into_iter().for_each(|(color, period)| {
                world
                    .spawn((Transform::IDENTITY, Spin::around(Vec3::UNIT_Y).period(period)))
                    .with_child((
                        Transform::from_translation(Vec3::new(4.5, 1.2, 0.0)),
                        PointLight { color, intensity: ch(9.0) },
                    ));
            });
        })
}

/// The SPEC-11 §7 "nova-roll" slice: a cube + an (emissive) cylinder, a
/// perspective camera, and one directional light — the smallest scene that
/// exercises `Mesh::cylinder` + an emissive material on both backends.
pub fn nova_roll_app() -> App {
    App::new()
        .window(Window::new(WIDTH, HEIGHT).with_clear_color(Color::linear_rgb(
            ch(0.02),
            ch(0.02),
            ch(0.05),
        )))
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let cylinder = meshes.add(Mesh::cylinder());
            let cube_mat =
                materials.add(Material::lit(Color::linear_rgb(ch(0.85), ch(0.30), ch(0.25))));
            let cyl_mat = materials.add(
                Material::lit(Color::linear_rgb(ch(0.25), ch(0.55), ch(0.90)))
                    .with_emissive(Color::linear_rgb(ch(0.6), ch(0.5), ch(0.1))),
            );
            world.spawn((
                Transform::from_translation(Vec3::new(-1.6, 0.0, 0.0)),
                Renderable { mesh: cube, material: cube_mat },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(1.6, 0.0, 0.0)),
                Renderable { mesh: cylinder, material: cyl_mat },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 6.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.4),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
}

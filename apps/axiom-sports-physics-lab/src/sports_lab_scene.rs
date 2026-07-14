//! The rendered scene: install-once statics (the procedurally textured field,
//! four walls, the sun + hemisphere ambient sky) and the per-frame dynamic
//! layer (camera, balls, the T-pose dummy's 15 posed boxes, and — in third
//! person — the player's own body). Everything visible is generated in code;
//! every non-decorative visible object has a physics twin in the core.

use axiom::prelude::{
    Angle, Camera, Color, DirectionalLight, Entity, Handle, Material, Mesh, PerspectiveProjection,
    RunningApp, Spawn, Transform, Vec3,
};
use axiom_host::FrameAmbient;
use axiom_kernel::{Meters, Ratio};
use axiom_math::Quat;

use super::sports_lab_app::{LabObjectKind, SportsPhysicsLab};
use super::sports_lab_balls::BallKind;
use super::sports_lab_camera::CameraMode;
use super::sports_lab_humanoid::{humanoid_figure, posed_boxes, ArmPose, TAG_COUNT};
use super::sports_lab_physics::{ARENA_HALF_L, ARENA_HALF_W, WALL_HEIGHT, WALL_THICKNESS};
use super::sports_lab_procgen;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("sports lab authored a finite ratio")
}

fn meters(v: f32) -> Meters {
    Meters::finite_or_zero(v)
}

fn color3(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ratio(rgb[0]), ratio(rgb[1]), ratio(rgb[2]))
}

/// Sky clear color (also the hemisphere ambient's sky half).
const SKY: [f32; 3] = [0.52, 0.70, 0.90];
const AMBIENT_GROUND: [f32; 3] = [0.30, 0.33, 0.28];

/// Dummy palette: shirt, shorts, skin, legs, shoes (indexed by part tag).
const DUMMY_PALETTE: [[f32; 3]; TAG_COUNT] = [
    [0.92, 0.44, 0.12], // shirt — safety orange
    [0.28, 0.28, 0.32], // shorts
    [0.85, 0.66, 0.46], // skin
    [0.44, 0.44, 0.48], // legs
    [0.16, 0.16, 0.18], // shoes
];

/// Player palette (same figure family, different kit).
const PLAYER_PALETTE: [[f32; 3]; TAG_COUNT] = [
    [0.16, 0.36, 0.86], // shirt — blue kit
    [0.10, 0.14, 0.32], // shorts
    [0.85, 0.66, 0.46], // skin
    [0.20, 0.24, 0.40], // legs
    [0.92, 0.92, 0.94], // shoes
];

/// The persistent render layer over a [`RunningApp`].
#[derive(Debug)]
pub struct SportsLabScene {
    sphere: Handle<Mesh>,
    cube: Handle<Mesh>,
    ball_mats: [Handle<Material>; 4],
    dummy_mats: [Handle<Material>; TAG_COUNT],
    player_mats: [Handle<Material>; TAG_COUNT],
    dummy_figure: axiom_figure::FigureDefinition,
    player_figure: axiom_figure::FigureDefinition,
    dynamic: Vec<Entity>,
}

impl SportsLabScene {
    /// Install the static scene (field, walls, lights, sky) and register every
    /// procedural mesh/texture/material the dynamic layer re-spawns per frame.
    pub fn install(app: &mut RunningApp) -> Self {
        let plane = app.add_mesh(Mesh::plane());
        let sphere = app.add_mesh(Mesh::sphere());
        let cube = app.add_mesh(Mesh::cube());

        // Procedural surfaces (baked in code — no assets).
        let field_tex = add_baked(app, sports_lab_procgen::field_texture());
        let ball_texes = [
            add_baked(app, sports_lab_procgen::soccer_texture()),
            add_baked(app, sports_lab_procgen::football_texture()),
            add_baked(app, sports_lab_procgen::bowling_texture()),
            add_baked(app, sports_lab_procgen::baseball_texture()),
        ];
        let field_mat =
            app.add_material(Material::lit(Color::WHITE).with_custom_texture(field_tex));
        let wall_mat = app.add_material(Material::lit(color3([0.62, 0.66, 0.72])));
        let ball_mats = [
            app.add_material(Material::lit(Color::WHITE).with_custom_texture(ball_texes[0])),
            app.add_material(Material::lit(Color::WHITE).with_custom_texture(ball_texes[1])),
            app.add_material(Material::lit(Color::WHITE).with_custom_texture(ball_texes[2])),
            app.add_material(Material::lit(Color::WHITE).with_custom_texture(ball_texes[3])),
        ];
        let palette_mats = |app: &mut RunningApp, palette: &[[f32; 3]; TAG_COUNT]| {
            [
                app.add_material(Material::lit(color3(palette[0]))),
                app.add_material(Material::lit(color3(palette[1]))),
                app.add_material(Material::lit(color3(palette[2]))),
                app.add_material(Material::lit(color3(palette[3]))),
                app.add_material(Material::lit(color3(palette[4]))),
            ]
        };
        let dummy_mats = palette_mats(app, &DUMMY_PALETTE);
        let player_mats = palette_mats(app, &PLAYER_PALETTE);

        // The field plane (60 × 90, markings in the texture).
        app.spawn(Spawn::new(
            Transform::new(
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::new(ARENA_HALF_W * 2.0, 1.0, ARENA_HALF_L * 2.0),
            ),
            plane,
            field_mat,
        ));

        // Four walls (visual twins of the physics half-spaces at their inner faces).
        let long = ARENA_HALF_L * 2.0 + WALL_THICKNESS * 2.0;
        let walls = [
            (
                Vec3::new(ARENA_HALF_W + WALL_THICKNESS * 0.5, WALL_HEIGHT * 0.5, 0.0),
                Vec3::new(WALL_THICKNESS, WALL_HEIGHT, long),
            ),
            (
                Vec3::new(
                    -(ARENA_HALF_W + WALL_THICKNESS * 0.5),
                    WALL_HEIGHT * 0.5,
                    0.0,
                ),
                Vec3::new(WALL_THICKNESS, WALL_HEIGHT, long),
            ),
            (
                Vec3::new(0.0, WALL_HEIGHT * 0.5, ARENA_HALF_L + WALL_THICKNESS * 0.5),
                Vec3::new(ARENA_HALF_W * 2.0, WALL_HEIGHT, WALL_THICKNESS),
            ),
            (
                Vec3::new(
                    0.0,
                    WALL_HEIGHT * 0.5,
                    -(ARENA_HALF_L + WALL_THICKNESS * 0.5),
                ),
                Vec3::new(ARENA_HALF_W * 2.0, WALL_HEIGHT, WALL_THICKNESS),
            ),
        ];
        for (center, scale) in walls {
            app.spawn(Spawn::new(
                Transform::new(center, Quat::IDENTITY, scale),
                cube,
                wall_mat,
            ));
        }

        // Sun + hemisphere sky.
        app.add_light(
            DirectionalLight {
                direction: Vec3::new(0.35, -1.0, 0.25),
                color: Color::WHITE,
                intensity: ratio(1.0),
            },
            Transform::IDENTITY,
        );
        app.set_ambient(FrameAmbient::new(SKY, AMBIENT_GROUND));

        SportsLabScene {
            sphere,
            cube,
            ball_mats,
            dummy_mats,
            player_mats,
            dummy_figure: humanoid_figure(ArmPose::TPose),
            player_figure: humanoid_figure(ArmPose::Lowered),
            dynamic: Vec::new(),
        }
    }

    /// Refresh the camera and re-spawn the dynamic layer for the lab's state.
    pub fn update(&mut self, app: &mut RunningApp, lab: &mut SportsPhysicsLab) {
        let (eye, target) = lab.camera_eye_target();
        let camera_pose = Transform::from_translation(eye)
            .looking_at(target, Vec3::UNIT_Y)
            .unwrap_or(Transform::from_translation(eye));
        app.set_camera(
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(66.0),
                near: meters(0.08),
                far: meters(400.0),
            }),
            camera_pose,
        );

        for entity in self.dynamic.drain(..) {
            app.despawn(entity);
        }

        let mut spawns: Vec<(Transform, Handle<Mesh>, Handle<Material>)> = Vec::new();
        for object in lab.objects() {
            let rot = Quat::new(object.rot[0], object.rot[1], object.rot[2], object.rot[3]);
            match object.kind {
                LabObjectKind::Ball(kind) => {
                    let mat = self.ball_mats[ball_mat_index(kind)];
                    spawns.push((
                        Transform::new(object.pos, rot, object.visual_scale),
                        self.sphere,
                        mat,
                    ));
                }
                LabObjectKind::Dummy => {
                    let body = Transform::new(object.pos, rot, Vec3::ONE);
                    for part in posed_boxes(&self.dummy_figure, body) {
                        spawns.push((
                            Transform::new(
                                part.transform.translation,
                                part.transform.rotation,
                                part.box_size,
                            ),
                            self.cube,
                            self.dummy_mats[part.tag as usize],
                        ));
                    }
                }
            }
        }

        // The player's own body, visible only from outside (first person would
        // put the head box through the near plane).
        if lab.camera_mode() == CameraMode::ThirdPerson {
            let body = lab.player().body_transform();
            for part in posed_boxes(&self.player_figure, body) {
                spawns.push((
                    Transform::new(
                        part.transform.translation,
                        part.transform.rotation,
                        part.box_size,
                    ),
                    self.cube,
                    self.player_mats[part.tag as usize],
                ));
            }
        }

        for (transform, mesh, material) in spawns {
            self.dynamic
                .push(app.spawn(Spawn::new(transform, mesh, material).casts_contact_shadow()));
        }
    }
}

/// Ball material slot (matches the install order above).
fn ball_mat_index(kind: BallKind) -> usize {
    match kind {
        BallKind::Soccer => 0,
        BallKind::Football => 1,
        BallKind::Bowling => 2,
        BallKind::Baseball => 3,
    }
}

fn add_baked(app: &mut RunningApp, baked: sports_lab_procgen::BakedTexture) -> u64 {
    app.add_texture_data(baked.width, baked.height, baked.pixels)
        .expect("baked lab texture is well-formed")
        .id()
}

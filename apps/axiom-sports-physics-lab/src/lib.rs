//! **Sports Physics Lab** — the foundational interactive sports primitive lab:
//! a first-person, fully procedural 3D practice arena proving the engine can
//! compose a playable sports test space from its existing public facades.
//!
//! One flat 60×90 field (procedurally textured with markings) enclosed by four
//! bouncy walls; a lineup of four procedural sports balls (soccer, football,
//! bowling, baseball — each a real `axiom-physics` dynamic body with its own
//! mass/restitution/friction); a T-pose humanoid dummy (a 15-part
//! `axiom-figure` box figure riding one rigid body); and a player who walks
//! (WASD + mouse-look via `axiom-fp-controller`), collides (kinematic sphere),
//! zooms between first and third person (V / wheel), picks objects up with the
//! reticle ray, carries them on a bounded-velocity drive, and tosses them with
//! mass-scaled impulse (left click; right click drops, R resets).
//!
//! Composition compromises forced by today's engine surface (each documented at
//! its site): the physics module has **no joints** (carry = velocity drive),
//! **no capsule contacts** (player = kinematic sphere), and **no convex/ellipsoid
//! colliders** (the football flies as a sphere, elongated visually); compound
//! rigid bodies don't exist, so the dummy is one box body wearing 15 render
//! boxes. All of it is app-tier glue over unmodified public facades.
//!
//! The core ([`SportsPhysicsLab`]) is engine-agnostic, deterministic, and
//! native-tested; the `wasm32` edge ([`web`]) decodes browser input, paints the
//! DOM HUD (reticle + labels), and presents through `run_web_multi`.

pub mod sports_lab_app;
pub mod sports_lab_balls;
pub mod sports_lab_camera;
pub mod sports_lab_humanoid;
pub mod sports_lab_interaction;
pub mod sports_lab_physics;
pub mod sports_lab_player;
pub mod sports_lab_procgen;
pub mod sports_lab_scene;

pub use sports_lab_app::{Intent, LabHud, LabObject, LabObjectKind, SportsPhysicsLab};
pub use sports_lab_camera::CameraMode;
pub use sports_lab_scene::SportsLabScene;

#[cfg(target_arch = "wasm32")]
pub mod overlay;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::sports_physics_lab_start;

use axiom::prelude::{App, Color, DefaultPlugins, RunningApp, Window};
use axiom_kernel::Ratio;

/// The canvas id the browser demo binds its surface to.
pub const CANVAS_ID: &str = "axiom-sports-physics-lab-canvas";

/// Surface size.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Live per-instance buffer capacity (statics + balls + two 15-part figures).
pub const LIVE_CAPACITY: u32 = 2048;

/// Build a live `RunningApp` with the scene installed and the dynamic layer
/// placed for the lab's current state.
pub fn live_app(lab: &mut SportsPhysicsLab) -> (RunningApp, SportsLabScene) {
    let ch = |v: f32| Ratio::new(v).expect("authored clear channel is finite");
    let mut running = App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(ch(0.52), ch(0.70), ch(0.90))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|_world, _meshes, _materials| {})
        .build();
    let mut scene = SportsLabScene::install(&mut running);
    scene.update(&mut running, lab);
    (running, scene)
}

/// Build the lab as a headless `RunningApp` for the native capture harness
/// (`axiom-shot`) and render tests: a fresh lab settled for one second so the
/// lineup rests on the field, framed from the first-person spawn.
pub fn build_sports_physics_lab() -> RunningApp {
    build_sports_physics_lab_posed(false)
}

/// The capture build with a view choice: `third_person` toggles the camera and
/// lets the orbit settle, so the harness can photograph the player's body
/// (`axiom-shot --app sports-physics-lab --frame 1`).
pub fn build_sports_physics_lab_posed(third_person: bool) -> RunningApp {
    let mut lab = SportsPhysicsLab::new();
    for _ in 0..60 {
        lab.step(Intent::default());
    }
    if third_person {
        lab.step(Intent { toggle_view: true, ..Intent::default() });
        for _ in 0..40 {
            lab.step(Intent::default());
        }
    }
    live_app(&mut lab).0
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 5] =
        ["Soccer Ball", "Football", "Bowling Ball", "Baseball", "Humanoid Dummy"];

    fn settled() -> SportsPhysicsLab {
        let mut lab = SportsPhysicsLab::new();
        for _ in 0..60 {
            lab.step(Intent::default());
        }
        lab
    }

    /// Step until the reticle hovers `name` (walking forward if needed).
    fn hover_named(lab: &mut SportsPhysicsLab, name: &str) {
        for _ in 0..600 {
            if lab.hud().hover == Some(name) {
                return;
            }
            // Aim at the object's current position, then close in.
            let target = lab.objects().iter().find(|o| o.name == name).unwrap().pos;
            let eye = lab.player().eye();
            let to = target.subtract(eye);
            let yaw_want = to.x.atan2(-to.z);
            let horiz = (to.x * to.x + to.z * to.z).sqrt();
            let pitch_want = to.y.atan2(horiz);
            let intent = Intent {
                forward: horiz > 2.5,
                look_yaw: yaw_want - lab.player().yaw(),
                look_pitch: pitch_want - lab.player().pitch(),
                ..Intent::default()
            };
            lab.step(intent);
        }
        panic!("never hovered {name}; hud = {:?}", lab.hud());
    }

    #[test]
    fn the_lab_contains_every_expected_object_with_a_physics_body() {
        let lab = SportsPhysicsLab::new();
        assert_eq!(lab.objects().len(), NAMES.len());
        for name in NAMES {
            let object = lab
                .objects()
                .iter()
                .find(|o| o.name == name)
                .unwrap_or_else(|| panic!("{name} exists"));
            assert!(object.body.is_valid(), "{name} has a physics body");
            assert!(object.pos.length().is_finite(), "{name} has a transform");
            assert!(object.grab_radius > 0.1, "{name} is targetable");
        }
    }

    #[test]
    fn the_settled_lineup_rests_on_the_field() {
        let lab = settled();
        for object in lab.objects() {
            assert!(
                object.pos.y > 0.05 && object.pos.y < 1.2,
                "{} rests on the field (not through it, not floating): y={}",
                object.name,
                object.pos.y
            );
            assert!(object.vel.length() < 0.8, "{} has settled", object.name);
        }
    }

    #[test]
    fn the_player_can_pick_up_and_toss_each_ball() {
        for name in ["Soccer Ball", "Football", "Bowling Ball", "Baseball"] {
            let mut lab = settled();
            hover_named(&mut lab, name);
            lab.step(Intent { primary: true, ..Intent::default() });
            assert_eq!(lab.hud().held, Some(name), "picked up {name}");

            // Carry for a moment: the object tracks the hold point.
            for _ in 0..40 {
                lab.step(Intent::default());
            }
            let idx = lab.objects().iter().position(|o| o.name == name).unwrap();
            let held_pos = lab.objects()[idx].pos;
            let eye = lab.player().eye();
            assert!(
                held_pos.subtract(eye).length() < 3.5,
                "{name} is carried near the player, at {held_pos:?}"
            );

            // Toss: it leaves the hand with velocity along the look direction.
            let look = lab.player().look_dir();
            lab.step(Intent { primary: true, ..Intent::default() });
            assert_eq!(lab.hud().held, None, "tossed {name}");
            let vel = lab.objects()[idx].vel;
            assert!(
                vel.dot(look) > 2.0,
                "{name} flies along the look direction: v={vel:?}"
            );
        }
    }

    #[test]
    fn only_one_object_is_held_at_a_time() {
        let mut lab = settled();
        hover_named(&mut lab, "Soccer Ball");
        lab.step(Intent { primary: true, ..Intent::default() });
        assert_eq!(lab.hud().held, Some("Soccer Ball"));
        // Look at another ball and click: it's a toss, not a second pickup.
        hover_named(&mut lab, "Bowling Ball");
        assert_eq!(lab.hud().held, Some("Soccer Ball"), "still holding the first ball");
        lab.step(Intent { primary: true, ..Intent::default() });
        assert_eq!(lab.hud().held, None, "the click tossed, never double-held");
    }

    #[test]
    fn the_toss_speed_scales_inversely_with_mass() {
        let speed_of = |name: &str| {
            let mut lab = settled();
            hover_named(&mut lab, name);
            lab.step(Intent { primary: true, ..Intent::default() });
            for _ in 0..30 {
                lab.step(Intent::default());
            }
            lab.step(Intent { primary: true, ..Intent::default() });
            let idx = lab.objects().iter().position(|o| o.name == name).unwrap();
            lab.objects()[idx].vel.length()
        };
        let baseball = speed_of("Baseball");
        let bowling = speed_of("Bowling Ball");
        assert!(
            baseball > bowling * 2.5,
            "the baseball leaves much faster than the bowling ball: {baseball} vs {bowling}"
        );
        assert!(bowling > 2.0, "even the bowling ball moves");
    }

    #[test]
    fn dropping_sets_the_object_down_gently() {
        let mut lab = settled();
        hover_named(&mut lab, "Soccer Ball");
        lab.step(Intent { primary: true, ..Intent::default() });
        for _ in 0..20 {
            lab.step(Intent::default());
        }
        lab.step(Intent { secondary: true, ..Intent::default() });
        assert_eq!(lab.hud().held, None);
        let ball = lab.objects().iter().find(|o| o.name == "Soccer Ball").unwrap();
        assert!(ball.vel.length() < 2.0, "a drop is gentle: v={:?}", ball.vel);
    }

    #[test]
    fn the_dummy_can_be_picked_up_and_tossed() {
        let mut lab = settled();
        hover_named(&mut lab, "Humanoid Dummy");
        lab.step(Intent { primary: true, ..Intent::default() });
        assert_eq!(lab.hud().held, Some("Humanoid Dummy"));
        lab.step(Intent { primary: true, ..Intent::default() });
        for _ in 0..10 {
            lab.step(Intent::default());
        }
        let dummy = lab.objects().iter().find(|o| o.name == "Humanoid Dummy").unwrap();
        assert!(dummy.vel.length() > 0.5, "the tossed dummy is moving");
    }

    #[test]
    fn reset_restores_the_lineup() {
        let mut lab = settled();
        hover_named(&mut lab, "Soccer Ball");
        lab.step(Intent { primary: true, ..Intent::default() });
        for _ in 0..30 {
            lab.step(Intent::default());
        }
        lab.step(Intent { primary: true, ..Intent::default() }); // toss it away
        for _ in 0..60 {
            lab.step(Intent::default());
        }
        lab.step(Intent { reset: true, ..Intent::default() });
        for object in lab.objects() {
            let d = object.pos.subtract(object.initial.translation).length();
            assert!(d < 0.05, "{} is back at its spawn (moved {d})", object.name);
        }
        assert_eq!(lab.hud().held, None, "reset empties the hands");
    }

    #[test]
    fn the_camera_toggles_between_first_and_third_person() {
        let mut lab = settled();
        assert_eq!(lab.camera_mode(), CameraMode::FirstPerson);
        let (fp_eye, _) = lab.camera_eye_target();
        assert!((fp_eye.y - 1.7).abs() < 1e-4, "first person sits at eye height");

        lab.step(Intent { toggle_view: true, ..Intent::default() });
        assert_eq!(lab.camera_mode(), CameraMode::ThirdPerson);
        for _ in 0..60 {
            lab.step(Intent::default());
        }
        let (tp_eye, _) = lab.camera_eye_target();
        let feet = lab.player().feet();
        assert!(tp_eye.subtract(feet).length() > 3.0, "third person pulls back");
        assert!(tp_eye.y > 1.9, "third person rises above the player");

        lab.step(Intent { toggle_view: true, ..Intent::default() });
        assert_eq!(lab.camera_mode(), CameraMode::FirstPerson);
        // Wheel zoom-out also enters third person.
        lab.step(Intent { zoom: 1.0, ..Intent::default() });
        assert_eq!(lab.camera_mode(), CameraMode::ThirdPerson);
    }

    #[test]
    fn two_identical_runs_are_deterministic() {
        let script = [
            Intent { forward: true, ..Intent::default() },
            Intent { forward: true, look_yaw: 0.02, ..Intent::default() },
            Intent { primary: true, ..Intent::default() },
            Intent { strafe_right: true, look_pitch: -0.01, ..Intent::default() },
        ];
        let run = || {
            let mut lab = SportsPhysicsLab::new();
            for i in 0..240 {
                lab.step(script[i % script.len()]);
            }
            (lab.state_digest(), lab.player().feet().x, lab.player().feet().z)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn the_lab_builds_a_renderable_app() {
        let mut app = build_sports_physics_lab();
        let outcome = app.tick(0);
        assert!(!outcome.draws().is_empty(), "the lab renders draws");
    }

    #[test]
    fn the_render_layer_shows_the_body_only_in_third_person() {
        let mut lab = settled();
        let (mut app, mut scene) = live_app(&mut lab);
        let fp_draws = app.tick(0).draws().len();
        lab.step(Intent { toggle_view: true, ..Intent::default() });
        scene.update(&mut app, &mut lab);
        let tp_draws = app.tick(1).draws().len();
        assert!(
            tp_draws >= fp_draws + sports_lab_humanoid::PART_COUNT,
            "third person adds the player's {} body parts: {fp_draws} -> {tp_draws}",
            sports_lab_humanoid::PART_COUNT
        );
    }

    #[test]
    fn tossed_balls_are_contained_by_the_walls() {
        let mut lab = settled();
        // Toss the baseball (fastest) straight at the far wall many times over.
        hover_named(&mut lab, "Baseball");
        lab.step(Intent { primary: true, ..Intent::default() });
        lab.step(Intent { primary: true, ..Intent::default() });
        for _ in 0..1200 {
            lab.step(Intent::default());
            let ball = lab.objects().iter().find(|o| o.name == "Baseball").unwrap();
            assert!(
                ball.pos.x.abs() < 30.5 && ball.pos.z.abs() < 45.5 && ball.pos.y > -0.5,
                "the baseball never escapes the arena: {:?}",
                ball.pos
            );
        }
    }

    #[test]
    fn velocities_never_exceed_the_safety_caps() {
        let mut lab = settled();
        hover_named(&mut lab, "Baseball");
        lab.step(Intent { primary: true, ..Intent::default() });
        lab.step(Intent { primary: true, ..Intent::default() });
        for _ in 0..300 {
            lab.step(Intent::default());
            for object in lab.objects() {
                assert!(object.vel.length() <= sports_lab_physics::MAX_LINEAR_SPEED + 1.0);
                assert!(object.ang.length() <= sports_lab_physics::MAX_ANGULAR_SPEED + 1.0);
            }
        }
    }

    #[test]
    fn the_hud_reports_the_lab_state() {
        let mut lab = settled();
        let hud = lab.hud();
        assert_eq!(hud.mode, CameraMode::FirstPerson);
        assert!(hud.step >= 60);
        assert!(hud.physics_step > 0, "the physics stepped");
        hover_named(&mut lab, "Soccer Ball");
        assert_eq!(lab.hud().hover, Some("Soccer Ball"));
        lab.step(Intent { primary: true, ..Intent::default() });
        assert_eq!(lab.hud().held, Some("Soccer Ball"));
    }

    #[test]
    fn scene_procgen_is_deterministic_for_a_fixed_seed() {
        // The full baked surface set is byte-identical across bakes (no RNG, no
        // wall clock — the recipes are pure functions of authored constants).
        let a = sports_lab_procgen::field_texture();
        let b = sports_lab_procgen::field_texture();
        assert_eq!(a.pixels, b.pixels);
        let digest = |lab: &SportsPhysicsLab| lab.state_digest();
        let l1 = SportsPhysicsLab::new();
        let l2 = SportsPhysicsLab::new();
        assert_eq!(digest(&l1), digest(&l2), "two fresh labs are identical");
    }
}

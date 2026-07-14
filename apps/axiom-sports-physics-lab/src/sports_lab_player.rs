//! The player: a first-person walk pose integrated through the engine's
//! `axiom-fp-controller` module (the sanctioned FP walk/look math), clamped to
//! the arena, and mirrored onto a kinematic physics sphere so balls collide
//! with the player's body. The pose also drives the visible third-person body
//! (a lowered-arms humanoid with a deterministic walk bob keyed to distance
//! travelled — no wall-clock time).

use axiom::prelude::Vec3;
use axiom_fp_controller::{FpController, LookDelta, MoveIntent, Pose, WalkTuning};
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{Quat, Transform};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};

use super::sports_lab_physics::{
    ARENA_HALF_L, ARENA_HALF_W, PLAYER_BODY_CENTER_Y, PLAYER_RADIUS, WALL_THICKNESS,
};
use super::Intent;

/// Eye height above the field (first-person camera seat).
pub const EYE_HEIGHT: f32 = 1.7;

/// Player spawn (near the field center, behind the ball lineup, facing it).
pub const SPAWN_X: f32 = 0.0;
pub const SPAWN_Z: f32 = 6.5;

/// Per-step walk speed (m / fixed step): a purposeful jog, ~5 m/s at 60 Hz.
const MOVE_SPEED: f32 = 0.085;

/// Walk-bob: vertical amplitude and cycle length (radians of phase per meter).
const BOB_AMPLITUDE: f32 = 0.045;
const BOB_FREQUENCY: f32 = 3.4;

/// The lab's walk tuning through the fp-controller vocabulary. Key-turn is
/// disabled (the mouse owns yaw); look sensitivity is applied at the web edge.
fn tuning() -> WalkTuning {
    WalkTuning::new(
        Meters::finite_or_zero(MOVE_SPEED),
        Radians::finite_or_zero(0.0),
        Meters::finite_or_zero(EYE_HEIGHT),
        Radians::finite_or_zero(1.45),
        Ratio::finite_or_zero(0.0022),
    )
}

/// The player rig: fp-controller pose + the kinematic collider + bob phase.
#[derive(Debug)]
pub struct PlayerRig {
    pose: Pose,
    pub body: PhysicsBodyHandle,
    /// Horizontal distance travelled (drives the deterministic walk bob).
    travel: f32,
    /// Whether the player moved this step (fades the bob when standing).
    moving: bool,
}

impl PlayerRig {
    /// A rig at the spawn, with its kinematic body already in `physics`.
    pub fn new(physics: &mut PhysicsApi) -> Self {
        let body = super::sports_lab_physics::add_player(physics, SPAWN_X, SPAWN_Z);
        PlayerRig {
            pose: Pose::new(
                Meters::finite_or_zero(SPAWN_X),
                Meters::finite_or_zero(SPAWN_Z),
                Radians::finite_or_zero(0.0),
                Radians::finite_or_zero(0.0),
            ),
            body,
            travel: 0.0,
            moving: false,
        }
    }

    /// Integrate one step of movement + look, clamp to the arena, and mirror the
    /// new position onto the kinematic body.
    pub fn step(&mut self, physics: &mut PhysicsApi, intent: &Intent) {
        let move_intent = MoveIntent {
            forward: intent.forward,
            backward: intent.backward,
            strafe_left: intent.strafe_left,
            strafe_right: intent.strafe_right,
            turn_left: false,
            turn_right: false,
        };
        let look = LookDelta::new(
            Radians::finite_or_zero(intent.look_yaw),
            Radians::finite_or_zero(intent.look_pitch),
        );
        let before = (self.pose.x().get(), self.pose.z().get());
        let next = FpController::step(self.pose, move_intent, look, tuning());

        // Keep the player inside the walls (margin: body radius + wall skin).
        let margin = PLAYER_RADIUS + WALL_THICKNESS * 0.5;
        let x = next
            .x()
            .get()
            .clamp(-(ARENA_HALF_W - margin), ARENA_HALF_W - margin);
        let z = next
            .z()
            .get()
            .clamp(-(ARENA_HALF_L - margin), ARENA_HALF_L - margin);
        self.pose = Pose::new(
            Meters::finite_or_zero(x),
            Meters::finite_or_zero(z),
            next.yaw(),
            next.pitch(),
        );

        let (dx, dz) = (x - before.0, z - before.1);
        let moved = (dx * dx + dz * dz).sqrt();
        self.moving = moved > 1e-5;
        self.travel += moved;

        physics
            .set_body_transform(
                self.body,
                Transform::from_translation(Vec3::new(x, PLAYER_BODY_CENTER_Y, z)),
            )
            .expect("player kinematic body follows the walk pose");
        // The solver never integrates a kinematic body's velocity, but it DOES
        // read it for contact impulses — storing the walk velocity is what makes
        // walking into a ball actually shove it.
        physics
            .set_body_velocity(
                self.body,
                Vec3::new(
                    dx / super::sports_lab_physics::DT,
                    0.0,
                    dz / super::sports_lab_physics::DT,
                ),
                Vec3::ZERO,
            )
            .expect("player kinematic body carries its walk velocity");
    }

    /// The planar feet position.
    pub fn feet(&self) -> Vec3 {
        Vec3::new(self.pose.x().get(), 0.0, self.pose.z().get())
    }

    /// The first-person eye position.
    pub fn eye(&self) -> Vec3 {
        Vec3::new(self.pose.x().get(), EYE_HEIGHT, self.pose.z().get())
    }

    /// The look yaw (radians about +Y; yaw 0 faces −Z).
    pub fn yaw(&self) -> f32 {
        self.pose.yaw().get()
    }

    /// The look pitch (radians about the view-right axis, clamped).
    pub fn pitch(&self) -> f32 {
        self.pose.pitch().get()
    }

    /// The full look direction (unit), pitch included.
    pub fn look_dir(&self) -> Vec3 {
        let (cp, sp) = (self.pitch().cos(), self.pitch().sin());
        Vec3::new(self.yaw().sin() * cp, sp, -self.yaw().cos() * cp)
    }

    /// The horizontal facing (unit) — where the body points.
    pub fn facing(&self) -> Vec3 {
        Vec3::new(self.yaw().sin(), 0.0, -self.yaw().cos())
    }

    /// The visible body's transform (the figure center): feet at the pose, a
    /// walk bob keyed to distance travelled, and the body turned to the facing.
    /// The figure's toes point +Z at identity, so the body yaw is `π − yaw`
    /// (mapping local +Z onto the facing `(sin yaw, 0, −cos yaw)`).
    pub fn body_transform(&self) -> Transform {
        let bob = if self.moving {
            (self.travel * BOB_FREQUENCY).sin().abs() * BOB_AMPLITUDE
        } else {
            0.0
        };
        let center = Vec3::new(
            self.pose.x().get(),
            super::sports_lab_humanoid::FIGURE_CENTER_Y + bob,
            self.pose.z().get(),
        );
        let body_yaw = core::f32::consts::PI - self.yaw();
        Transform::new(center, Quat::from_euler_xyz(0.0, body_yaw, 0.0), Vec3::ONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports_lab_physics;

    fn rig() -> (PhysicsApi, PlayerRig) {
        let mut physics = sports_lab_physics::world();
        sports_lab_physics::add_arena(&mut physics);
        let rig = PlayerRig::new(&mut physics);
        (physics, rig)
    }

    fn intent() -> Intent {
        Intent::default()
    }

    #[test]
    fn the_player_spawns_at_eye_height_facing_the_lineup() {
        let (_physics, rig) = rig();
        assert_eq!(rig.eye().y, EYE_HEIGHT);
        // Yaw 0 faces −Z: toward the ball lineup at z ≈ 2.5 from spawn z 6.5.
        let f = rig.facing();
        assert!(f.z < -0.99, "spawn faces −Z, got {f:?}");
    }

    #[test]
    fn walking_forward_moves_along_the_look_direction() {
        let (mut physics, mut rig) = rig();
        let start = rig.feet();
        for _ in 0..60 {
            rig.step(
                &mut physics,
                &Intent {
                    forward: true,
                    ..intent()
                },
            );
        }
        let moved = rig.feet().subtract(start);
        assert!(
            moved.z < -3.0,
            "one second of forward walk covers ground, got {moved:?}"
        );
        assert!(
            rig.body_transform().translation.y > 0.9,
            "the body bobs above the ground"
        );
    }

    #[test]
    fn mouse_look_turns_yaw_and_clamps_pitch() {
        let (mut physics, mut rig) = rig();
        rig.step(
            &mut physics,
            &Intent {
                look_yaw: 0.5,
                look_pitch: 9.0,
                ..intent()
            },
        );
        assert!((rig.yaw() - 0.5).abs() < 1e-6);
        assert!(
            (rig.pitch() - 1.45).abs() < 1e-6,
            "pitch clamps to the tuning limit"
        );
        // Look direction follows both axes and stays unit length.
        let d = rig.look_dir();
        assert!((d.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_arena_walls_clamp_the_walk() {
        let (mut physics, mut rig) = rig();
        // Face +X (yaw π/2) and hold forward for a long time.
        rig.step(
            &mut physics,
            &Intent {
                look_yaw: core::f32::consts::FRAC_PI_2,
                ..intent()
            },
        );
        for _ in 0..3000 {
            rig.step(
                &mut physics,
                &Intent {
                    forward: true,
                    ..intent()
                },
            );
        }
        assert!(
            rig.feet().x < ARENA_HALF_W - PLAYER_RADIUS + 0.01,
            "the wall stopped the player"
        );
        assert!(
            rig.feet().x > ARENA_HALF_W - 2.0,
            "the player reached the wall"
        );
    }

    #[test]
    fn the_kinematic_body_follows_the_pose() {
        let (mut physics, mut rig) = rig();
        for _ in 0..30 {
            rig.step(
                &mut physics,
                &Intent {
                    forward: true,
                    ..intent()
                },
            );
        }
        let snap = physics.snapshot();
        let body = snap
            .bodies()
            .iter()
            .find(|b| b.handle() == rig.body)
            .unwrap();
        let t = body.transform().translation;
        assert!((t.x - rig.feet().x).abs() < 1e-4 && (t.z - rig.feet().z).abs() < 1e-4);
        assert_eq!(t.y, PLAYER_BODY_CENTER_Y);
    }

    #[test]
    fn walking_into_a_ball_pushes_it() {
        let mut physics = sports_lab_physics::world();
        sports_lab_physics::add_arena(&mut physics);
        let mut rig = PlayerRig::new(&mut physics);
        // Put a soccer ball directly in the walk path.
        let mut preset = crate::sports_lab_balls::BALLS[0];
        preset.spawn = Vec3::new(SPAWN_X, preset.radius, SPAWN_Z - 2.0);
        let ball = sports_lab_physics::add_ball(&mut physics, &preset);
        for n in 0..240 {
            rig.step(
                &mut physics,
                &Intent {
                    forward: true,
                    ..Intent::default()
                },
            );
            physics
                .step(sports_lab_physics::runtime_step(n))
                .expect("step");
        }
        let snap = physics.snapshot();
        let b = snap.bodies().iter().find(|b| b.handle() == ball).unwrap();
        assert!(
            b.transform().translation.z < SPAWN_Z - 2.6,
            "the player's body shoved the ball forward, z={}",
            b.transform().translation.z
        );
    }
}

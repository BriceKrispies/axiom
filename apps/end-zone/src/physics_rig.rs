//! The simulation's physics composition over the engine's `PhysicsApi`
//! facade: one deterministic world holding the field ground plane, the
//! football (a dynamic sphere — the engine has no prolate collider; the
//! silhouette is visual), and one kinematic sphere per player mirrored from
//! the controller each tick so a loose ball interacts with bodies.
//!
//! Physics faults are recorded, never panicked on (`unwrap`/`expect`-free).

use axiom::prelude::{Transform, Vec3};
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::config::{FIXED_STEP_NANOS, PLAYER_COUNT};
use crate::football::state::BALL_RADIUS;
use crate::player::PlayerSim;

/// Height of a player's kinematic sphere center above the ground, yards.
const PLAYER_BODY_Y: f32 = 1.0;
/// Player kinematic sphere radius, yards.
const PLAYER_BODY_RADIUS: f32 = 0.55;

fn ratio(v: f32) -> Ratio {
    Ratio::finite_or_zero(v)
}

fn meters(v: f32) -> Meters {
    Meters::finite_or_zero(v)
}

/// The `RuntimeStep` for fixed step `n`.
pub fn runtime_step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), FIXED_STEP_NANOS, n)
}

/// The physics world plus the handles the simulation drives.
#[derive(Debug)]
pub struct PhysicsRig {
    pub physics: PhysicsApi,
    pub ball: PhysicsBodyHandle,
    pub players: Vec<PhysicsBodyHandle>,
    /// First physics fault, if any (subsystem: operation). Inspectable in the
    /// debug overlay; the sim never panics on a physics error.
    pub fault: Option<&'static str>,
}

impl PhysicsRig {
    /// Build the world: gravity in yd/s², turf plane, ball, player spheres.
    pub fn new(gravity: f32, ball_spawn: Vec3) -> Self {
        let mut physics = PhysicsApi::with_config(
            Vec3::new(0.0, -gravity, 0.0),
            8,
            PLAYER_COUNT as u32 + 8,
            PLAYER_COUNT as u32 + 8,
            4,
            true,
            ratio(0.0008),
            ratio(0.004),
        )
        .unwrap_or_else(|_| PhysicsApi::new());

        let mut fault = None;
        let note = |slot: &mut Option<&'static str>, op: &'static str, ok: bool| {
            if !ok && slot.is_none() {
                *slot = Some(op);
            }
        };

        let turf = PhysicsApi::material(ratio(0.65), ratio(0.42), ratio(1.0));
        let leather = PhysicsApi::material(ratio(0.5), ratio(0.5), ratio(1.0));
        let body_mat = PhysicsApi::material(ratio(0.5), ratio(0.3), ratio(1.0));

        let ground = physics.create_static_body(Transform::IDENTITY);
        let ground_ok = match (ground, turf) {
            (Ok(body), Ok(mat)) => physics
                .attach_plane_collider(body, Vec3::UNIT_Y, meters(0.0), mat, false)
                .is_ok(),
            _ => false,
        };
        note(&mut fault, "physics_rig: ground plane", ground_ok);

        let ball =
            physics.create_dynamic_body(Transform::from_translation(ball_spawn), ratio(0.42));
        let ball_handle = match (ball, leather) {
            (Ok(body), Ok(mat)) => {
                let attached = physics
                    .attach_sphere_collider(body, meters(BALL_RADIUS), mat, false)
                    .is_ok();
                note(&mut fault, "physics_rig: ball collider", attached);
                body
            }
            _ => {
                note(&mut fault, "physics_rig: ball body", false);
                PhysicsBodyHandle::default()
            }
        };

        let mut players = Vec::with_capacity(PLAYER_COUNT);
        for index in 0..PLAYER_COUNT {
            let spawn = Transform::from_translation(Vec3::new(
                index as f32 * 2.0 - 13.0,
                PLAYER_BODY_Y,
                0.0,
            ));
            let handle = match (physics.create_kinematic_body(spawn), body_mat) {
                (Ok(body), Ok(mat)) => {
                    let attached = physics
                        .attach_sphere_collider(body, meters(PLAYER_BODY_RADIUS), mat, false)
                        .is_ok();
                    note(&mut fault, "physics_rig: player collider", attached);
                    body
                }
                _ => {
                    note(&mut fault, "physics_rig: player body", false);
                    PhysicsBodyHandle::default()
                }
            };
            players.push(handle);
        }

        PhysicsRig {
            physics,
            ball: ball_handle,
            players,
            fault,
        }
    }

    /// Record the first fault from a physics operation.
    pub fn note(&mut self, op: &'static str, ok: bool) {
        if !ok && self.fault.is_none() {
            self.fault = Some(op);
        }
    }

    /// Mirror the authoritative player kinematics into their kinematic bodies.
    pub fn mirror_players(&mut self, players: &[PlayerSim]) {
        for (index, player) in players.iter().enumerate() {
            let handle = self.players[index];
            let pose = Transform::from_translation(Vec3::new(
                player.pos.x,
                player.pos.y + PLAYER_BODY_Y,
                player.pos.z,
            ));
            let moved = self.physics.set_body_transform(handle, pose).is_ok();
            let pushed = self
                .physics
                .set_body_velocity(handle, player.vel, Vec3::ZERO)
                .is_ok();
            self.note("physics_rig: mirror player", moved && pushed);
        }
    }

    /// Step the world for tick `n` and drain its event log (bounded).
    pub fn step(&mut self, n: u64) {
        let ok = self.physics.step(runtime_step(n)).is_ok();
        self.note("physics_rig: step", ok);
        let _ = self.physics.drain_events();
    }

    /// The ball body's `(position, linear velocity)` from the latest snapshot.
    pub fn ball_state(&self) -> Option<(Vec3, Vec3)> {
        let snapshot = self.physics.snapshot();
        snapshot
            .bodies()
            .iter()
            .find(|b| b.handle() == self.ball)
            .map(|b| (b.transform().translation, b.linear_velocity()))
    }

    /// Launch the ball as a live projectile from `pos` with velocity `vel`.
    pub fn launch_ball(&mut self, pos: Vec3, vel: Vec3, spin: Vec3) {
        let placed = self
            .physics
            .set_body_transform(self.ball, Transform::from_translation(pos))
            .is_ok();
        let thrown = self.physics.set_body_velocity(self.ball, vel, spin).is_ok();
        let enabled = self.physics.enable_body(self.ball).is_ok();
        self.note("physics_rig: launch ball", placed && thrown && enabled);
    }

    /// Park the ball body (held/dead — the sim owns its position).
    pub fn park_ball(&mut self) {
        let ok = self.physics.disable_body(self.ball).is_ok();
        self.note("physics_rig: park ball", ok);
    }
}

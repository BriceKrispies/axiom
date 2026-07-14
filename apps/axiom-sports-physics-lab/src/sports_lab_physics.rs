//! The lab's physics composition: one `axiom-physics` world holding the arena
//! (a static floor plane + four static wall planes), the four dynamic ball
//! spheres, the dynamic dummy box, and the kinematic player sphere — plus every
//! tuning constant. Contact support in the physics module dictates the collider
//! choices: spheres and boxes collide against planes and spheres, so the arena
//! bounds are half-space planes (not boxes) and the player is a kinematic
//! sphere (capsule colliders exist but generate no contacts yet).

use axiom::prelude::{Transform, Vec3};
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::Quat;
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use super::sports_lab_balls::BallPreset;
use super::sports_lab_humanoid::{DUMMY_BOX_HALF_EXTENTS, FIGURE_CENTER_Y};

// --- simulation ---------------------------------------------------------------

/// Earth-like gravity.
pub const GRAVITY: Vec3 = Vec3::new(0.0, -9.8, 0.0);

/// Fixed simulation step: 60 Hz, in nanoseconds for the runtime step.
pub const FIXED_STEP_NANOS: u64 = 16_666_667;

/// Seconds per fixed step.
pub const DT: f32 = FIXED_STEP_NANOS as f32 / 1_000_000_000.0;

/// Substeps per fixed step — keeps a fast-tossed baseball from tunnelling a wall.
pub const MAX_SUBSTEPS: u32 = 4;

/// Sequential-impulse solver iterations.
pub const SOLVER_ITERATIONS: u32 = 10;

/// Per-step linear velocity decay (air drag; also settles rest jitter).
pub const LINEAR_DAMPING: f32 = 0.0008;

/// Per-step angular velocity decay (rolling resistance).
pub const ANGULAR_DAMPING: f32 = 0.003;

/// Safety caps: no body may exceed these speeds (app-side clamp per step). The
/// angular cap must clear a small ball's natural rolling rate (`v / r` — the
/// baseball rolls at ~140 rad/s at full toss speed), so it is a numeric guard,
/// not a gameplay limit.
pub const MAX_LINEAR_SPEED: f32 = 40.0;
pub const MAX_ANGULAR_SPEED: f32 = 160.0;

// --- arena ----------------------------------------------------------------------

/// Arena half extents: the field is 60 wide (±x) by 90 long (±z).
pub const ARENA_HALF_W: f32 = 30.0;
pub const ARENA_HALF_L: f32 = 45.0;

/// Visible wall height / thickness (the physics walls are infinite half-spaces
/// at the walls' inner faces, so nothing ever leaves the arena).
pub const WALL_HEIGHT: f32 = 3.4;
pub const WALL_THICKNESS: f32 = 0.6;

/// Field surface material.
pub const FIELD_FRICTION: f32 = 0.8;
pub const FIELD_RESTITUTION: f32 = 0.3;

/// Wall material — bouncy enough that tossed balls rebound into play.
pub const WALL_FRICTION: f32 = 0.35;
pub const WALL_RESTITUTION: f32 = 0.55;

// --- characters -------------------------------------------------------------------

/// Player collider: a kinematic sphere at chest height (see module docs).
pub const PLAYER_RADIUS: f32 = 0.38;
pub const PLAYER_BODY_CENTER_Y: f32 = 0.95;

/// Dummy rigid body.
pub const DUMMY_MASS: f32 = 4.0;
pub const DUMMY_FRICTION: f32 = 0.6;
pub const DUMMY_RESTITUTION: f32 = 0.25;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("sports lab authored a finite ratio")
}

fn meters(v: f32) -> Meters {
    Meters::finite_or_zero(v)
}

/// The `RuntimeStep` for fixed step `n`.
pub fn runtime_step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), FIXED_STEP_NANOS, n)
}

/// A fresh physics world with the lab's gravity, solver, and damping tuning.
pub fn world() -> PhysicsApi {
    PhysicsApi::with_config(
        GRAVITY,
        SOLVER_ITERATIONS,
        64,
        64,
        MAX_SUBSTEPS,
        true,
        ratio(LINEAR_DAMPING),
        ratio(ANGULAR_DAMPING),
    )
    .expect("valid sports-lab physics config")
}

/// Install the arena: a floor half-space at y=0 and four inward-facing wall
/// half-spaces at the arena bounds. Solid side is `n·x ≥ distance`, matching
/// the floor's `(0,1,0), 0` convention, so each wall keeps bodies inside.
pub fn add_arena(physics: &mut PhysicsApi) {
    let field_mat =
        PhysicsApi::material(ratio(FIELD_FRICTION), ratio(FIELD_RESTITUTION), ratio(1.0))
            .expect("field material");
    let wall_mat = PhysicsApi::material(ratio(WALL_FRICTION), ratio(WALL_RESTITUTION), ratio(1.0))
        .expect("wall material");

    let floor = physics
        .create_static_body(Transform::IDENTITY)
        .expect("floor body");
    physics
        .attach_plane_collider(
            floor,
            Vec3::new(0.0, 1.0, 0.0),
            meters(0.0),
            field_mat,
            false,
        )
        .expect("floor plane collider");

    // (inward normal, distance): keeps n·p ≥ distance, i.e. the body inside.
    let walls = [
        (Vec3::new(-1.0, 0.0, 0.0), -ARENA_HALF_W), // x ≤ +30
        (Vec3::new(1.0, 0.0, 0.0), -ARENA_HALF_W),  // x ≥ −30
        (Vec3::new(0.0, 0.0, -1.0), -ARENA_HALF_L), // z ≤ +45
        (Vec3::new(0.0, 0.0, 1.0), -ARENA_HALF_L),  // z ≥ −45
    ];
    for (normal, distance) in walls {
        let body = physics
            .create_static_body(Transform::IDENTITY)
            .expect("wall body");
        physics
            .attach_plane_collider(body, normal, meters(distance), wall_mat, false)
            .expect("wall plane collider");
    }
}

/// Add one ball as a dynamic sphere at its spawn transform.
pub fn add_ball(physics: &mut PhysicsApi, preset: &BallPreset) -> PhysicsBodyHandle {
    let material = PhysicsApi::material(
        ratio(preset.friction),
        ratio(preset.restitution),
        ratio(1.0),
    )
    .expect("ball material");
    let body = physics
        .create_dynamic_body(ball_spawn_transform(preset), ratio(preset.mass))
        .expect("ball body");
    physics
        .attach_sphere_collider(body, meters(preset.radius), material, false)
        .expect("ball collider");
    body
}

/// A ball's initial transform (position + the football's lie-down pitch).
pub fn ball_spawn_transform(preset: &BallPreset) -> Transform {
    Transform::new(
        preset.spawn,
        Quat::from_euler_xyz(preset.spawn_pitch, 0.0, 0.0),
        Vec3::ONE,
    )
}

/// Add the T-pose dummy as one dynamic box body standing at `(x, z)`.
pub fn add_dummy(physics: &mut PhysicsApi, x: f32, z: f32) -> PhysicsBodyHandle {
    let material =
        PhysicsApi::material(ratio(DUMMY_FRICTION), ratio(DUMMY_RESTITUTION), ratio(1.0))
            .expect("dummy material");
    let body = physics
        .create_dynamic_body(dummy_spawn_transform(x, z), ratio(DUMMY_MASS))
        .expect("dummy body");
    physics
        .attach_box_collider(body, DUMMY_BOX_HALF_EXTENTS, material, false)
        .expect("dummy collider");
    body
}

/// The dummy's initial standing transform at `(x, z)`.
pub fn dummy_spawn_transform(x: f32, z: f32) -> Transform {
    Transform::from_translation(Vec3::new(x, FIGURE_CENTER_Y, z))
}

/// Add the player as a kinematic sphere (immovable to the solver — balls bounce
/// off it; the walk pose drives it via `set_body_transform` each step).
pub fn add_player(physics: &mut PhysicsApi, x: f32, z: f32) -> PhysicsBodyHandle {
    let material =
        PhysicsApi::material(ratio(0.5), ratio(0.3), ratio(1.0)).expect("player material");
    let body = physics
        .create_kinematic_body(Transform::from_translation(Vec3::new(
            x,
            PLAYER_BODY_CENTER_Y,
            z,
        )))
        .expect("player body");
    physics
        .attach_sphere_collider(body, meters(PLAYER_RADIUS), material, false)
        .expect("player collider");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports_lab_balls::BALLS;

    #[test]
    fn a_dropped_ball_falls_bounces_and_comes_to_rest_on_the_field() {
        let mut physics = world();
        add_arena(&mut physics);
        let mut preset = BALLS[0];
        preset.spawn = Vec3::new(0.0, 3.0, 0.0);
        let ball = add_ball(&mut physics, &preset);
        let mut max_after_drop = 0.0f32;
        let mut y = 3.0f32;
        for n in 0..600 {
            physics.step(runtime_step(n)).expect("step");
            let snap = physics.snapshot();
            let b = snap.bodies().iter().find(|b| b.handle() == ball).unwrap();
            y = b.transform().translation.y;
            if n > 80 {
                max_after_drop = max_after_drop.max(y);
            }
        }
        // It never fell through, it bounced (rose again after first contact),
        // and it ends resting near its radius.
        assert!(
            max_after_drop > preset.radius + 0.05,
            "the soccer ball bounced"
        );
        assert!(
            (y - preset.radius).abs() < 0.15,
            "ball rests on the field, y={y}"
        );
    }

    #[test]
    fn a_fast_ball_rebounds_off_the_arena_wall() {
        let mut physics = world();
        add_arena(&mut physics);
        let preset = BALLS[0];
        let ball = add_ball(&mut physics, &preset);
        physics
            .set_body_velocity(ball, Vec3::new(20.0, 0.0, 0.0), Vec3::ZERO)
            .expect("launch at +x wall");
        let mut min_vx = f32::MAX;
        let mut max_x = f32::MIN;
        for n in 0..240 {
            physics.step(runtime_step(n)).expect("step");
            let snap = physics.snapshot();
            let b = snap.bodies().iter().find(|b| b.handle() == ball).unwrap();
            min_vx = min_vx.min(b.linear_velocity().x);
            max_x = max_x.max(b.transform().translation.x);
        }
        assert!(
            max_x <= ARENA_HALF_W + 0.01,
            "the wall contained the ball, x={max_x}"
        );
        assert!(
            min_vx < -3.0,
            "the ball rebounded (negative x velocity), vx={min_vx}"
        );
    }

    #[test]
    fn the_dummy_stands_and_the_player_body_is_immovable() {
        let mut physics = world();
        add_arena(&mut physics);
        let dummy = add_dummy(&mut physics, 4.0, -3.0);
        let player = add_player(&mut physics, 0.0, 6.0);
        for n in 0..120 {
            physics.step(runtime_step(n)).expect("step");
        }
        let snap = physics.snapshot();
        let d = snap.bodies().iter().find(|b| b.handle() == dummy).unwrap();
        assert!(
            (d.transform().translation.y - FIGURE_CENTER_Y).abs() < 0.1,
            "dummy stands"
        );
        let p = snap.bodies().iter().find(|b| b.handle() == player).unwrap();
        assert_eq!(
            p.transform().translation.y,
            PLAYER_BODY_CENTER_Y,
            "kinematic player holds"
        );
    }

    #[test]
    fn balls_collide_with_each_other() {
        let mut physics = world();
        add_arena(&mut physics);
        let mut a = BALLS[0];
        a.spawn = Vec3::new(-2.0, a.radius, 0.0);
        let mut b = BALLS[2];
        b.spawn = Vec3::new(2.0, b.radius, 0.0);
        let ball_a = add_ball(&mut physics, &a);
        let ball_b = add_ball(&mut physics, &b);
        physics
            .set_body_velocity(ball_a, Vec3::new(8.0, 0.0, 0.0), Vec3::ZERO)
            .expect("roll the soccer ball at the bowling ball");
        // (`latest_contacts` reports only the final substep, so the impact is
        // asserted through its physical outcome, not the contact report.)
        let mut min_vx = f32::MAX;
        for n in 0..300 {
            physics.step(runtime_step(n)).expect("step");
            let snap = physics.snapshot();
            let pa = snap.bodies().iter().find(|x| x.handle() == ball_a).unwrap();
            min_vx = min_vx.min(pa.linear_velocity().x);
        }
        let snap = physics.snapshot();
        let bb = snap.bodies().iter().find(|x| x.handle() == ball_b).unwrap();
        assert!(
            bb.transform().translation.x > 2.05,
            "the bowling ball was shoved"
        );
        assert!(
            min_vx < -0.2,
            "the light soccer ball rebounded off the heavy one"
        );
    }
}

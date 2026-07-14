//! Deterministic steering primitives: arrival, teammate separation, pursuit
//! prediction, and the turn-rate/acceleration-limited velocity update the
//! player controller integrates. Pure functions of explicit inputs — no state,
//! no randomness.

use axiom::prelude::Vec3;

use crate::data::{BehaviorTuning, PlayerArchetype};

/// Flatten to the ground plane.
fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The desired ground velocity to reach `target` from `pos`: full speed
/// outside the arrival radius, proportionally slower inside it.
pub fn arrive(pos: Vec3, target: Vec3, max_speed: f32, arrival_radius: f32) -> Vec3 {
    let to = flat(target.subtract(pos));
    let distance = to.length();
    if distance < 1.0e-4 {
        return Vec3::ZERO;
    }
    let speed = if distance < arrival_radius {
        max_speed * (distance / arrival_radius)
    } else {
        max_speed
    };
    to.mul_scalar(speed / distance)
}

/// Separation push away from nearby teammates (positions + radii), weighted by
/// overlap depth. Deterministic: neighbors are visited in caller order
/// (ascending player id).
pub fn separation(
    pos: Vec3,
    own_radius: f32,
    neighbors: &[(Vec3, f32)],
    tuning: &BehaviorTuning,
) -> Vec3 {
    let mut push = Vec3::ZERO;
    for (other, radius) in neighbors {
        let away = flat(pos.subtract(*other));
        let distance = away.length();
        let range = own_radius + radius + tuning.separation_radius;
        if distance > 1.0e-4 && distance < range {
            let weight = (range - distance) / range;
            push = push.add(away.mul_scalar(weight / distance));
        }
    }
    push.mul_scalar(tuning.separation_strength)
}

/// Predict where a moving target will be `seconds` from now (constant-velocity
/// extrapolation — the pursuit lead defenders aim at).
pub fn predict(target_pos: Vec3, target_vel: Vec3, seconds: f32) -> Vec3 {
    target_pos.add(flat(target_vel).mul_scalar(seconds))
}

/// The pursuit lead time for an archetype: distance-scaled, bounded, and
/// scaled by the archetype's aggressiveness (0 = no lead, 1 = full lead).
pub fn pursuit_lead_seconds(distance: f32, archetype: &PlayerArchetype) -> f32 {
    let base = (distance / archetype.max_speed.max(0.1)).min(0.9);
    base * archetype.pursuit_aggressiveness
}

/// One tick's velocity update under acceleration + turn-rate limits.
///
/// The heading may rotate at most `turn_rate * dt` toward the desired heading,
/// and the speed may change by at most `acceleration * dt`. Returns the new
/// ground velocity.
pub fn limited_velocity_update(
    current_vel: Vec3,
    desired_vel: Vec3,
    archetype: &PlayerArchetype,
    dt: f32,
) -> Vec3 {
    let current = flat(current_vel);
    let desired = flat(desired_vel);
    let current_speed = current.length();
    let desired_speed = desired.length().min(archetype.max_speed);

    // Heading: rotate the current heading toward the desired one, clamped.
    let new_heading = if desired_speed < 1.0e-4 {
        current
    } else if current_speed < 0.25 {
        // Nearly stopped: free to face any way (no meaningful momentum).
        desired
    } else {
        let ch = current.mul_scalar(1.0 / current_speed);
        let dh = desired.mul_scalar(1.0 / desired_speed);
        let dot = (ch.x * dh.x + ch.z * dh.z).clamp(-1.0, 1.0);
        let angle = dot.acos();
        let max_turn = archetype.turn_rate * dt;
        if angle <= max_turn {
            dh
        } else {
            // Rotate `ch` by ±max_turn toward `dh` (sign from the cross-y).
            let cross_y = ch.z * dh.x - ch.x * dh.z;
            let sign = if cross_y >= 0.0 { 1.0 } else { -1.0 };
            let (s, c) = (sign * max_turn).sin_cos();
            Vec3::new(ch.x * c + ch.z * s, 0.0, -ch.x * s + ch.z * c)
        }
    };

    // Speed: approach the desired speed under the acceleration limit.
    let max_delta = archetype.acceleration * dt;
    let new_speed = if desired_speed > current_speed {
        (current_speed + max_delta).min(desired_speed)
    } else {
        (current_speed - max_delta).max(desired_speed)
    };

    let heading_len = new_heading.length();
    if heading_len < 1.0e-4 {
        Vec3::ZERO
    } else {
        new_heading.mul_scalar(new_speed / heading_len)
    }
}

/// The yaw (radians, `0` faces `+Z`) of a ground direction, or `fallback`
/// when the direction is degenerate.
pub fn yaw_of(direction: Vec3, fallback: f32) -> f32 {
    let d = flat(direction);
    if d.length() < 1.0e-4 {
        fallback
    } else {
        d.x.atan2(d.z)
    }
}

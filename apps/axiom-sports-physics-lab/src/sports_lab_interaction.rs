//! Interaction: reticle-ray targeting, one-object pickup, carrying, tossing,
//! and gentle dropping. Targeting is an app-side ray-vs-bounding-sphere sweep
//! over the lab's interactables (the physics module's raycast treats boxes as
//! axis-aligned and this needs per-object grab radii anyway). A carried object
//! **stays a live dynamic body**: the physics module has no joints, so the
//! carry is a bounded-velocity drive toward the hold point — the body still
//! collides with walls and the field, cannot tunnel, and cannot explode
//! (velocity magnitude is capped every step).

use axiom::prelude::Vec3;
use axiom_physics::PhysicsApi;

use super::sports_lab_app::LabObject;
use super::sports_lab_physics::{ARENA_HALF_L, ARENA_HALF_W, DT, WALL_THICKNESS};

/// How far the reticle ray reaches for pickups.
pub const REACH: f32 = 3.8;

/// Hold point distance in front of the eye.
pub const HOLD_DISTANCE: f32 = 2.2;

/// Cap on the carry drive velocity (m/s) — bounded by design.
const CARRY_MAX_SPEED: f32 = 14.0;

/// Per-step decay of a carried object's angular velocity (it settles in hand).
const CARRY_SPIN_DECAY: f32 = 0.8;

/// Toss momentum (mass × speed): heavier objects leave the hand slower.
const TOSS_MOMENTUM: f32 = 6.5;

/// Toss speed clamp: even the bowling ball moves; even the baseball stays sane.
const TOSS_SPEED_MIN: f32 = 3.5;
const TOSS_SPEED_MAX: f32 = 17.0;

/// Tumble imparted alongside a toss (rad/s per m/s of toss speed).
const TOSS_TUMBLE: f32 = 0.55;

/// Gentle drop: a slight forward set-down.
const DROP_SPEED: f32 = 1.0;

/// Pickup/carry state: at most one object is ever held.
#[derive(Debug, Default)]
pub struct InteractionState {
    held: Option<usize>,
    hover: Option<usize>,
}

impl InteractionState {
    /// Index of the held object, if any.
    pub fn held(&self) -> Option<usize> {
        self.held
    }

    /// Index of the hovered (reticle-targeted) object, if any.
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    /// Re-target: the nearest interactable whose grab sphere the eye ray enters
    /// within [`REACH`]. The held object is never its own target.
    pub fn update_hover(&mut self, eye: Vec3, look: Vec3, objects: &[LabObject]) {
        self.hover = objects
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != self.held)
            .filter_map(|(i, o)| ray_sphere(eye, look, o.pos, o.grab_radius).map(|t| (i, t)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal))
            .map(|(i, _)| i);
    }

    /// Primary action: empty-handed picks up the hovered object; holding one
    /// tosses it along the look direction with mass-scaled speed and a tumble.
    pub fn primary(&mut self, physics: &mut PhysicsApi, objects: &[LabObject], look: Vec3) {
        match self.held {
            Some(i) => {
                let o = &objects[i];
                let speed = (TOSS_MOMENTUM / o.mass).clamp(TOSS_SPEED_MIN, TOSS_SPEED_MAX);
                // Tumble about the horizontal right axis → forward end-over-end.
                let right = Vec3::new(-look.z, 0.0, look.x);
                physics
                    .set_body_velocity(
                        o.body,
                        look.mul_scalar(speed),
                        right.mul_scalar(speed * TOSS_TUMBLE),
                    )
                    .expect("toss the held object");
                self.held = None;
            }
            None => {
                if let Some(i) = self.hover {
                    self.held = Some(i);
                    self.hover = None;
                }
            }
        }
    }

    /// Secondary action: set the held object down gently just ahead.
    pub fn secondary(&mut self, physics: &mut PhysicsApi, objects: &[LabObject], look: Vec3) {
        if let Some(i) = self.held.take() {
            let forward = Vec3::new(look.x, 0.0, look.z);
            physics
                .set_body_velocity(objects[i].body, forward.mul_scalar(DROP_SPEED), Vec3::ZERO)
                .expect("drop the held object");
        }
    }

    /// Force-release without imparting motion (scene reset).
    pub fn release(&mut self) {
        self.held = None;
        self.hover = None;
    }

    /// Drive the held object toward the hold point with a bounded velocity.
    /// The hold point is clamped above the field and inside the walls so the
    /// carry never presses the body through a boundary.
    pub fn drive_held(
        &mut self,
        physics: &mut PhysicsApi,
        objects: &[LabObject],
        eye: Vec3,
        look: Vec3,
    ) {
        let Some(i) = self.held else { return };
        let o = &objects[i];
        let mut hold = eye.add(look.mul_scalar(HOLD_DISTANCE));
        let margin = o.grab_radius.min(0.6) + WALL_THICKNESS * 0.5;
        hold.y = hold.y.max(o.grab_radius.min(0.5) + 0.05);
        hold.x = hold
            .x
            .clamp(-(ARENA_HALF_W - margin), ARENA_HALF_W - margin);
        hold.z = hold
            .z
            .clamp(-(ARENA_HALF_L - margin), ARENA_HALF_L - margin);

        let mut velocity = hold.subtract(o.pos).mul_scalar(1.0 / DT);
        let speed = velocity.length();
        if speed > CARRY_MAX_SPEED {
            velocity = velocity.mul_scalar(CARRY_MAX_SPEED / speed);
        }
        physics
            .set_body_velocity(o.body, velocity, o.ang.mul_scalar(CARRY_SPIN_DECAY))
            .expect("carry drive");
    }
}

/// Ray/sphere intersection: the entry distance `t ≥ 0` along unit `dir` from
/// `origin` into the sphere `(center, radius)`, within [`REACH`]; a ray starting
/// inside the sphere hits at `t = 0`.
fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin.subtract(center);
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let sqrt_disc = disc.sqrt();
    let t_enter = -b - sqrt_disc;
    let t_exit = -b + sqrt_disc;
    if t_exit < 0.0 {
        return None; // sphere entirely behind the eye
    }
    let t = t_enter.max(0.0);
    (t <= REACH).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_sphere_hits_misses_and_handles_inside() {
        let dir = Vec3::new(0.0, 0.0, -1.0);
        // Dead-ahead hit at distance 2 − radius.
        let t = ray_sphere(Vec3::ZERO, dir, Vec3::new(0.0, 0.0, -2.0), 0.5).unwrap();
        assert!((t - 1.5).abs() < 1e-4);
        // Off to the side: miss.
        assert!(ray_sphere(Vec3::ZERO, dir, Vec3::new(3.0, 0.0, -2.0), 0.5).is_none());
        // Behind: miss.
        assert!(ray_sphere(Vec3::ZERO, dir, Vec3::new(0.0, 0.0, 2.0), 0.5).is_none());
        // Beyond reach: miss.
        assert!(ray_sphere(Vec3::ZERO, dir, Vec3::new(0.0, 0.0, -(REACH + 2.0)), 0.5).is_none());
        // Starting inside: immediate hit.
        assert_eq!(
            ray_sphere(Vec3::ZERO, dir, Vec3::new(0.0, 0.0, -0.1), 0.5),
            Some(0.0)
        );
    }
}

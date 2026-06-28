//! Deterministic per-body force / impulse / torque accumulation.

use axiom_math::Vec3;

/// The forces staged on a rigid body between steps.
///
/// `force` is a continuous force integrated over the step (it produces a linear
/// acceleration `force * inverse_mass`); `impulse` is an instantaneous change
/// applied once at the next step (a velocity change `impulse * inverse_mass`);
/// `torque` is the angular analogue of `force` — a continuous torque integrated
/// over the step into an angular acceleration `inverse_inertia ⊙ torque`.
/// [`ForceAccumulator::clear`] resets all three after each step, so forces never
/// silently persist across steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ForceAccumulator {
    force: Vec3,
    impulse: Vec3,
    torque: Vec3,
}

impl ForceAccumulator {
    /// A cleared accumulator (all components zero).
    pub(crate) fn new() -> Self {
        ForceAccumulator {
            force: Vec3::ZERO,
            impulse: Vec3::ZERO,
            torque: Vec3::ZERO,
        }
    }

    /// Add a continuous force (applied over the step during integration).
    pub(crate) fn apply_force(&mut self, force: Vec3) {
        self.force = self.force.add(force);
    }

    /// Add an instantaneous impulse (applied once at the next step).
    pub(crate) fn apply_impulse(&mut self, impulse: Vec3) {
        self.impulse = self.impulse.add(impulse);
    }

    /// Add a continuous torque (applied over the step during integration, the
    /// angular analogue of [`ForceAccumulator::apply_force`]).
    pub(crate) fn apply_torque(&mut self, torque: Vec3) {
        self.torque = self.torque.add(torque);
    }

    /// Reset every accumulated force, impulse, and torque to zero.
    pub(crate) fn clear(&mut self) {
        self.force = Vec3::ZERO;
        self.impulse = Vec3::ZERO;
        self.torque = Vec3::ZERO;
    }

    /// The accumulated continuous force.
    pub(crate) fn force(&self) -> Vec3 {
        self.force
    }

    /// The accumulated instantaneous impulse.
    pub(crate) fn impulse(&self) -> Vec3 {
        self.impulse
    }

    /// The accumulated continuous torque.
    pub(crate) fn torque(&self) -> Vec3 {
        self.torque
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_zeroed() {
        let a = ForceAccumulator::new();
        assert_eq!(a.force(), Vec3::ZERO);
        assert_eq!(a.impulse(), Vec3::ZERO);
        assert_eq!(a.torque(), Vec3::ZERO);
    }

    #[test]
    fn forces_impulses_and_torques_accumulate_additively() {
        let mut a = ForceAccumulator::new();
        a.apply_force(Vec3::new(1.0, 0.0, 0.0));
        a.apply_force(Vec3::new(0.0, 2.0, 0.0));
        a.apply_impulse(Vec3::new(0.0, 0.0, 3.0));
        a.apply_impulse(Vec3::new(0.0, 0.0, 1.0));
        a.apply_torque(Vec3::new(0.5, 0.0, 0.0));
        a.apply_torque(Vec3::new(0.0, 0.5, 0.0));
        assert_eq!(a.force(), Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(a.impulse(), Vec3::new(0.0, 0.0, 4.0));
        assert_eq!(a.torque(), Vec3::new(0.5, 0.5, 0.0));
    }

    #[test]
    fn clear_resets_everything() {
        let mut a = ForceAccumulator::new();
        a.apply_force(Vec3::new(5.0, 5.0, 5.0));
        a.apply_impulse(Vec3::new(5.0, 5.0, 5.0));
        a.apply_torque(Vec3::new(5.0, 5.0, 5.0));
        a.clear();
        assert_eq!(a.force(), Vec3::ZERO);
        assert_eq!(a.impulse(), Vec3::ZERO);
        assert_eq!(a.torque(), Vec3::ZERO);
    }

    #[test]
    fn derives_are_exercised() {
        let a = ForceAccumulator::new();
        let b = a;
        assert_eq!(a, b);
        let mut c = a;
        c.apply_force(Vec3::ONE);
        assert_ne!(a, c);
        assert!(format!("{a:?}").contains("ForceAccumulator"));
    }
}

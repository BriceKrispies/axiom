//! A procedural-animation component an app spawns onto a node: a data-declared
//! bob (and optional spin) the engine animates each tick, around the node's
//! resting transform.

use axiom_kernel::Meters;
use axiom_math::Vec3;

/// A procedural animation an app attaches to a node: a bob of `bob_amplitude`
/// along +Y every `bob_period` ticks, plus an optional spin about `spin_axis`
/// every `spin_period` ticks, offset by `phase`. The engine's procedural-animation
/// system animates it deterministically from the frame tick, composed **around the
/// node's resting (spawn) transform** — so a *positioned* node (a wall cube at a
/// grid cell) comes alive in place. `phase` offsets the bob so a whole scene of
/// nodes never pulses in lockstep; an app draws that variety (per-node phase /
/// period) from the procedural-generation substrate or a node's grid position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcAnim {
    pub bob_amplitude: Meters,
    pub bob_period: u32,
    pub spin_axis: Vec3,
    pub spin_period: u32,
    pub phase: u32,
}

impl ProcAnim {
    /// A bob of `amplitude` along +Y every `period_ticks` frames, with no spin
    /// (add one with [`Self::spin`]) and no phase offset.
    pub const fn bob(amplitude: Meters, period_ticks: u32) -> Self {
        ProcAnim {
            bob_amplitude: amplitude,
            bob_period: period_ticks,
            spin_axis: Vec3::ZERO,
            spin_period: 1,
            phase: 0,
        }
    }

    /// Add a spin about `axis`, one full revolution every `period_ticks` frames.
    pub const fn spin(self, axis: Vec3, period_ticks: u32) -> Self {
        ProcAnim {
            bob_amplitude: self.bob_amplitude,
            bob_period: self.bob_period,
            spin_axis: axis,
            spin_period: period_ticks,
            phase: self.phase,
        }
    }

    /// Offset the bob by `phase` ticks so neighbouring nodes never animate in
    /// lockstep.
    pub const fn phase(self, phase: u32) -> Self {
        ProcAnim {
            bob_amplitude: self.bob_amplitude,
            bob_period: self.bob_period,
            spin_axis: self.spin_axis,
            spin_period: self.spin_period,
            phase,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bob_then_spin_then_phase_builds_the_animation() {
        let a = ProcAnim::bob(Meters::new(0.5).unwrap(), 120)
            .spin(Vec3::UNIT_Y, 240)
            .phase(30);
        assert_eq!(a.bob_amplitude.get(), 0.5);
        assert_eq!(a.bob_period, 120);
        assert_eq!(a.spin_axis, Vec3::UNIT_Y);
        assert_eq!(a.spin_period, 240);
        assert_eq!(a.phase, 30);
    }

    #[test]
    fn bob_defaults_to_no_spin_and_no_phase() {
        let a = ProcAnim::bob(Meters::new(1.0).unwrap(), 60);
        assert_eq!(a.spin_axis, Vec3::ZERO);
        assert_eq!(a.spin_period, 1);
        assert_eq!(a.phase, 0);
    }
}

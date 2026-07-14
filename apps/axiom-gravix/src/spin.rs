//! The **Sonic-style brake / spin-launch** mechanic, as an explicit deterministic
//! state machine: `NormalRolling → Braking → SpinCharging → LaunchRelease`.
//!
//! - Hold **Shift** → `Braking`: the ball slows to a stop via a physics-friendly
//!   velocity decay (the game applies it — this machine only *asks* for it), not a
//!   position lock.
//! - Braked and nearly stopped, **tap a move key** → `SpinCharging`: repeated taps
//!   add charge up to a cap, the latest tapped (camera-relative) direction becomes
//!   the launch heading, and the ball visibly spins in place. Charge decays slowly
//!   if tapping stops (and drops back to `Braking` if it fully bleeds off).
//! - **Release Shift** → `LaunchRelease`: the stored charge is emitted as a launch
//!   in the charged direction, then the machine returns to `NormalRolling`. No
//!   charge → it just resumes rolling.
//!
//! The controller is pure and deterministic (no wall-clock, no randomness): the
//! same input frames always drive the same states — see the replay test. The game
//! reads [`SpinOutput`] each step and turns it into physics (brake decay, in-place
//! spin, launch velocity).

use axiom::prelude::Vec3;

use crate::settings;

/// The four explicit states of the spin-launch machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinState {
    NormalRolling,
    Braking,
    SpinCharging,
    LaunchRelease,
}

/// What the controller asks the game to do this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpinOutput {
    /// Apply the physics-friendly brake velocity decay this step.
    pub braking: bool,
    /// Normal camera-relative movement drive is allowed this step.
    pub allow_move: bool,
    /// Visible in-place spin (angular speed, rad/s) while charging; `0` otherwise.
    pub spin_visual: f32,
    /// On release: `(unit launch direction, charge in 0..=1)` to convert into
    /// launch linear + angular velocity. `None` on non-release steps.
    pub launch: Option<(Vec3, f32)>,
    /// The state after this step (for the HUD / debug / tests).
    pub state: SpinState,
    /// The stored charge after this step.
    pub charge: f32,
}

/// The deterministic brake / spin-launch controller.
#[derive(Debug, Clone, Copy)]
pub struct SpinController {
    state: SpinState,
    charge: f32,
    dir: Vec3,
}

impl SpinController {
    /// A fresh controller in `NormalRolling` with no charge.
    pub fn new() -> Self {
        SpinController { state: SpinState::NormalRolling, charge: 0.0, dir: Vec3::new(0.0, 0.0, 1.0) }
    }

    /// The current state.
    pub fn state(&self) -> SpinState {
        self.state
    }

    /// The current stored charge (`0..=SPIN_CHARGE_MAX`).
    pub fn charge(&self) -> f32 {
        self.charge
    }

    /// Advance one fixed step. `brake` is Shift-held; `tap` is `Some(dir)` when a
    /// move key was *pressed this step* (edge-triggered, camera-relative unit
    /// direction); `speed` is the ball's horizontal speed.
    pub fn update(&mut self, brake: bool, tap: Option<Vec3>, speed: f32, dt: f32) -> SpinOutput {
        let mut launch = None;
        match self.state {
            SpinState::NormalRolling => {
                if brake {
                    self.state = SpinState::Braking;
                }
            }
            SpinState::Braking => {
                if !brake {
                    self.state = SpinState::NormalRolling;
                } else if speed < settings::SPIN_STOP_SPEED {
                    if let Some(d) = tap {
                        self.dir = horizontal_unit(d);
                        self.charge = (self.charge + settings::SPIN_CHARGE_PER_TAP).min(settings::SPIN_CHARGE_MAX);
                        self.state = SpinState::SpinCharging;
                    }
                }
            }
            SpinState::SpinCharging => {
                if !brake {
                    let has = self.charge > 0.0;
                    launch = has.then_some((self.dir, self.charge));
                    self.charge = 0.0;
                    self.state = if has { SpinState::LaunchRelease } else { SpinState::NormalRolling };
                } else if let Some(d) = tap {
                    self.dir = horizontal_unit(d);
                    self.charge = (self.charge + settings::SPIN_CHARGE_PER_TAP).min(settings::SPIN_CHARGE_MAX);
                } else {
                    self.charge = (self.charge - settings::SPIN_CHARGE_DECAY * dt).max(0.0);
                    if self.charge <= 0.0 {
                        self.state = SpinState::Braking;
                    }
                }
            }
            SpinState::LaunchRelease => {
                self.state = SpinState::NormalRolling;
            }
        }

        SpinOutput {
            braking: matches!(self.state, SpinState::Braking | SpinState::SpinCharging),
            allow_move: matches!(self.state, SpinState::NormalRolling),
            spin_visual: match self.state {
                SpinState::SpinCharging => self.charge * settings::SPIN_CHARGE_VISUAL,
                _ => 0.0,
            },
            launch,
            state: self.state,
            charge: self.charge,
        }
    }
}

impl Default for SpinController {
    fn default() -> Self {
        SpinController::new()
    }
}

/// A horizontal unit vector (Y flattened), defaulting to `+Z` when degenerate.
fn horizontal_unit(v: Vec3) -> Vec3 {
    let len = (v.x * v.x + v.z * v.z).sqrt();
    if len < 1.0e-5 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(v.x / len, 0.0, v.z / len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const FWD: Vec3 = Vec3::new(0.0, 0.0, 1.0);
    const RIGHT: Vec3 = Vec3::new(1.0, 0.0, 0.0);

    #[test]
    fn brake_enters_braking_and_release_resumes() {
        let mut c = SpinController::new();
        let out = c.update(true, None, 10.0, DT); // moving fast, brake held
        assert_eq!(out.state, SpinState::Braking);
        assert!(out.braking && !out.allow_move);
        // Releasing with no charge returns straight to rolling.
        let out = c.update(false, None, 8.0, DT);
        assert_eq!(out.state, SpinState::NormalRolling);
        assert!(out.allow_move && out.launch.is_none());
    }

    #[test]
    fn tapping_while_braked_and_stopped_charges_up_to_the_cap() {
        let mut c = SpinController::new();
        c.update(true, None, 10.0, DT); // -> Braking
        // Nearly stopped + tap forward -> SpinCharging, charge rises.
        let a = c.update(true, Some(FWD), 0.5, DT);
        assert_eq!(a.state, SpinState::SpinCharging);
        assert!((a.charge - settings::SPIN_CHARGE_PER_TAP).abs() < 1.0e-6);
        assert!(a.spin_visual > 0.0, "the ball visibly spins while charging");
        // Many taps cap the charge.
        for _ in 0..50 {
            c.update(true, Some(FWD), 0.2, DT);
        }
        assert!((c.charge() - settings::SPIN_CHARGE_MAX).abs() < 1.0e-6);
    }

    #[test]
    fn charge_decays_when_not_tapping_and_falls_back_to_braking() {
        let mut c = SpinController::new();
        c.update(true, None, 10.0, DT);
        c.update(true, Some(FWD), 0.3, DT); // charging, one tap
        let charged = c.charge();
        // A step with no tap decays the charge.
        let out = c.update(true, None, 0.3, DT);
        assert!(out.charge < charged, "charge decays without taps: {} < {}", out.charge, charged);
        // Enough idle steps bleed it off entirely, dropping back to Braking.
        for _ in 0..600 {
            c.update(true, None, 0.3, DT);
        }
        assert_eq!(c.state(), SpinState::Braking);
        assert_eq!(c.charge(), 0.0);
    }

    #[test]
    fn releasing_a_charge_launches_in_the_charged_direction_then_resets() {
        let mut c = SpinController::new();
        c.update(true, None, 10.0, DT);
        c.update(true, Some(RIGHT), 0.3, DT); // charge toward +x
        c.update(true, Some(RIGHT), 0.3, DT); // more charge
        let stored = c.charge();
        // Release -> launch in +x with the stored charge; charge resets.
        let out = c.update(false, None, 0.3, DT);
        let (dir, charge) = out.launch.expect("a charged release launches");
        assert!((dir.subtract(RIGHT)).length() < 1.0e-6, "launches along the charged direction");
        assert!((charge - stored).abs() < 1.0e-6);
        assert_eq!(out.state, SpinState::LaunchRelease);
        assert_eq!(c.charge(), 0.0);
        // The next step is transient and returns to rolling with no launch.
        let after = c.update(false, None, 5.0, DT);
        assert_eq!(after.state, SpinState::NormalRolling);
        assert!(after.launch.is_none() && after.allow_move);
    }

    #[test]
    fn the_machine_replays_deterministically_over_fixed_input_frames() {
        // A fixed script of (brake, tap) frames drives identical state sequences.
        let script: Vec<(bool, Option<Vec3>)> = vec![
            (false, None),
            (true, None),
            (true, Some(FWD)),
            (true, Some(FWD)),
            (true, None),
            (false, None),
            (false, None),
        ];
        let run = || {
            let mut c = SpinController::new();
            script.iter().map(|&(b, t)| c.update(b, t, 0.4, DT).state).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
        // And the launch fires exactly on the release-with-charge frame (index 5).
        let mut c = SpinController::new();
        let launches: Vec<bool> = script.iter().map(|&(b, t)| c.update(b, t, 0.4, DT).launch.is_some()).collect();
        assert_eq!(launches, vec![false, false, false, false, false, true, false]);
    }
}

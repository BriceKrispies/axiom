//! [`DriveCommand`] — the one thing the simulation is allowed to read from the
//! outside world.
//!
//! Every input path converges here before the simulation sees anything: the
//! keyboard (via [`axiom_input::InputState`]'s action table), the gamepad's
//! analogue axes, the scripted test drivers, and the capture harness all produce
//! a `DriveCommand` per fixed step and nothing else. A step's behaviour is
//! therefore a pure function of `(state, DriveCommand)`, which is what makes
//! "the same ordered input sequence replays identically" true rather than hoped
//! for.
//!
//! The analogue shape (`f32` throttle rather than `bool` accelerating) is
//! deliberate: a keyboard produces `0.0`/`1.0`, a trigger produces everything in
//! between, and the controller does not need to know which it is talking to.

/// One fixed step's worth of driver intent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DriveCommand {
    /// Throttle, `0..1`.
    pub throttle: f32,
    /// Brake / reverse, `0..1`.
    pub brake: f32,
    /// Steering, `-1` (left) to `+1` (right).
    pub steer: f32,
    /// A lane change: `-1` one lane left, `+1` one lane right, `0` none.
    ///
    /// **Edge-triggered by the caller**, exactly like [`Self::reset`]: it is one
    /// hop, not a held direction, so a finger resting on the LEFT button moves
    /// the car one lane and no further. Only [`crate::PlayProfile::Rails`] reads
    /// it; the wheel game steers with [`Self::steer`] and ignores this entirely.
    /// It lives on the same command as everything else because the simulation
    /// reads exactly one input type — a second command struct for the phone
    /// would be a second way for input to reach the sim, and the replay
    /// guarantee rests on there being only one.
    pub lane_step: i8,
    /// Handbrake held.
    pub handbrake: bool,
    /// Boost held.
    pub boost: bool,
    /// Reset to the last safe point (edge-triggered by the caller).
    pub reset: bool,
    /// Pause / resume toggle (edge-triggered by the caller).
    pub pause: bool,
    /// Restart the whole run (edge-triggered by the caller).
    pub restart: bool,
}

impl DriveCommand {
    /// Coasting: no input at all.
    pub const IDLE: DriveCommand = DriveCommand {
        throttle: 0.0,
        brake: 0.0,
        steer: 0.0,
        lane_step: 0,
        handbrake: false,
        boost: false,
        reset: false,
        pause: false,
        restart: false,
    };

    /// Flat out in a straight line — the command the countdown hands the car and
    /// the one most scripted tests start from.
    pub const FLAT_OUT: DriveCommand = DriveCommand {
        throttle: 1.0,
        ..DriveCommand::IDLE
    };

    /// Throttle with a steering input.
    pub const fn turning(steer: f32) -> DriveCommand {
        DriveCommand {
            throttle: 1.0,
            steer,
            ..DriveCommand::IDLE
        }
    }

    /// The same intent, with every **one-shot** channel spent.
    ///
    /// A browser frame is not a fixed step. When one frame banks two or three
    /// steps, the app runs them all under the command the player gave *once* —
    /// which is right for the held channels (a finger on the throttle means
    /// throttle for the whole frame) and wrong for every edge-triggered one. A
    /// tap is one tap however many steps the frame turned out to be worth.
    ///
    /// This is the single definition of which channels those are, and it lives
    /// here beside the fields rather than at the call sites that iterate. It was
    /// a hand-copied list at two of them, and `lane_step` was added to the struct
    /// without being added to either — so on any frame long enough to bank two
    /// steps, one tap of LEFT moved the car two lanes. Which frames those are is
    /// not a fixed property of the game: it moved with the render cost, so the
    /// bug arrived with a heavier frame rather than with the code that caused it.
    ///
    /// Adding a one-shot channel means adding it here, once.
    pub fn spent(self) -> DriveCommand {
        DriveCommand {
            lane_step: 0,
            reset: false,
            pause: false,
            restart: false,
            ..self
        }
    }

    /// Clamp every analogue channel into its legal range. The simulation calls
    /// this on entry so a misbehaving input source can never inject a `NaN` or
    /// an out-of-range axis into the deterministic state.
    pub fn sanitised(self) -> DriveCommand {
        DriveCommand {
            throttle: finite(self.throttle).clamp(0.0, 1.0),
            brake: finite(self.brake).clamp(0.0, 1.0),
            steer: finite(self.steer).clamp(-1.0, 1.0),
            // One hop per command, whatever a caller asks for: the rails solver
            // treats this as a lane delta and a value of 7 would teleport the car
            // across the road.
            lane_step: self.lane_step.clamp(-1, 1),
            ..self
        }
    }
}

/// A finite value, or zero. The single guard that keeps a non-finite input from
/// reaching the integrator.
fn finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_idle_command_asks_for_nothing() {
        let c = DriveCommand::IDLE;
        assert_eq!(c.throttle, 0.0);
        assert_eq!(c.brake, 0.0);
        assert_eq!(c.steer, 0.0);
        assert_eq!(c.lane_step, 0);
        assert!(!c.handbrake && !c.boost && !c.reset && !c.pause && !c.restart);
        assert_eq!(DriveCommand::default(), c);
    }

    #[test]
    fn the_convenience_commands_are_what_they_claim() {
        assert_eq!(DriveCommand::FLAT_OUT.throttle, 1.0);
        assert_eq!(DriveCommand::FLAT_OUT.steer, 0.0);
        let right = DriveCommand::turning(0.8);
        assert_eq!(right.throttle, 1.0);
        assert_eq!(right.steer, 0.8);
    }

    #[test]
    fn sanitising_clamps_every_analogue_channel() {
        let wild = DriveCommand {
            throttle: 4.0,
            brake: -2.0,
            steer: 9.0,
            lane_step: 7,
            handbrake: true,
            boost: true,
            reset: true,
            pause: true,
            restart: true,
        };
        let clean = wild.sanitised();
        assert_eq!(clean.throttle, 1.0);
        assert_eq!(clean.brake, 0.0);
        assert_eq!(clean.steer, 1.0);
        // A lane delta is one hop, however many a caller asked for.
        assert_eq!(clean.lane_step, 1);
        // The digital channels pass through untouched.
        assert!(clean.handbrake && clean.boost && clean.reset && clean.pause && clean.restart);
    }

    #[test]
    fn sanitising_replaces_non_finite_input_with_zero() {
        let broken = DriveCommand {
            throttle: f32::NAN,
            brake: f32::INFINITY,
            steer: f32::NEG_INFINITY,
            ..DriveCommand::IDLE
        };
        let clean = broken.sanitised();
        assert_eq!(clean.throttle, 0.0);
        assert_eq!(clean.brake, 0.0);
        assert_eq!(clean.steer, 0.0);
    }

    #[test]
    fn sanitising_clamps_a_lane_hop_in_both_directions() {
        let far_left = DriveCommand {
            lane_step: -9,
            ..DriveCommand::IDLE
        };
        assert_eq!(far_left.sanitised().lane_step, -1);
        let single = DriveCommand {
            lane_step: -1,
            ..DriveCommand::IDLE
        };
        assert_eq!(single.sanitised().lane_step, -1);
    }

    #[test]
    fn sanitising_leaves_a_legal_command_alone() {
        let legal = DriveCommand {
            throttle: 0.6,
            brake: 0.25,
            steer: -0.4,
            ..DriveCommand::IDLE
        };
        assert_eq!(legal.sanitised(), legal);
    }
}

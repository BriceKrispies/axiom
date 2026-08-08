//! Executing a commitment: the keeper's body, moving.
//!
//! Split from the state machine next door because it is a different kind of
//! thing. [`super`] decides *what* the keeper is trying to do — when it reads,
//! what it commits to, whether it gets its one correction. This decides how a
//! body gets there, and the answer is: with momentum.
//!
//! The dive is **integrated**, not parameterised. Position and velocity;
//! accelerate toward whatever is currently committed to, capped at the keeper's
//! own top speed. The correction moves the *target* and never touches the body.
//!
//! That is not a stylistic preference, it is the fix for a real defect. The dive
//! used to be `home + (aim - home) * smoothstep(elapsed / extend_time)`, and the
//! correction reset `home` to wherever the hips were and restarted `elapsed` at
//! zero. Smoothstep begins at zero velocity, so a keeper mid-dive at full speed
//! was instantaneously stopped and made to accelerate again: it committed a bit,
//! stopped dead, then carried on — a visible stutter every time it corrected. A
//! body with momentum cannot do that whatever its target does, because velocity
//! is carried across the change rather than rebuilt from it.
//!
//! Everything else the pose reads — the bank, the height bias, the point the
//! hands are thrown at — eases toward the commitment for the same reason. Taking
//! the stutter out of the legs and leaving it in the arms would not be a fix.

use axiom::prelude::Vec3;

use crate::figure::KeeperMotion;
use crate::play::keeper_read::KeeperRead;
use crate::tuning::KeeperTuning;

use super::{Keeper, HIP_HEIGHT};

impl Keeper {
    /// The small bounce a keeper does on the line before a penalty.
    ///
    /// **Vertical only, and deliberately so.** It used to sway sideways as well,
    /// which is what a keeper does — but "sideways" is an absolute direction and
    /// the sway is a function of the clock, so it was the same way round for a
    /// shot and for that shot's mirror image. The keeper therefore began its dive
    /// from a slightly different place relative to the ball depending on which
    /// way the ball went, and about one marginal penalty in a hundred came out
    /// differently from its own reflection. A game that is symmetric everywhere
    /// else cannot afford one asymmetric flourish in the idle.
    ///
    /// It is presentation, but it comes out of the same motion value the capsules
    /// are built from — so a keeper who has drifted is genuinely there, and
    /// anything it does has to be true.
    pub(super) fn set_stance(&self, t: f32) -> KeeperMotion {
        let bounce = (t * 9.0).sin();
        let hips = Vec3::new(
            self.home.x,
            self.home.y - bounce.abs() * 0.05,
            self.home.z,
        );
        KeeperMotion {
            hips,
            lean: 0.0,
            extend: 0.0,
            height_bias: 0.0,
            // Set, hands up and forward: the shape a keeper holds on the line.
            hands: Vec3::new(hips.x, hips.y + 0.35, hips.z + 0.30),
        }
    }

    /// Execute the committed dive, one step of it.
    ///
    /// The hips are a body under acceleration toward the committed target, capped
    /// at the keeper's own top speed. The correction next door changes the target
    /// and nothing else, so the movement stays continuous through it — see the
    /// module docs for the stutter this replaced.
    pub(super) fn dive_step(&mut self, read: KeeperRead, dt: f32, tuning: &KeeperTuning) -> KeeperMotion {
        let target = Vec3::new(
            read.aim.x,
            HIP_HEIGHT + (read.aim.y - HIP_HEIGHT).max(0.0)
                - (HIP_HEIGHT - read.aim.y).max(0.0) * 0.55,
            self.home.z - 0.16,
        );
        let to_go = target.subtract(self.motion.hips);
        // The speed it wants: flat out toward the target, except over the last
        // few centimetres, where wanting to arrive at 8 m/s would just oscillate.
        let wanted = to_go
            .normalize()
            .map(|d| d.mul_scalar(tuning.dive_speed.min(to_go.length() / dt.max(1.0e-3))))
            .unwrap_or(Vec3::ZERO);
        // `extend_time` is now what it always physically was: how long the keeper
        // takes to get its body up to that speed.
        let respond = (dt / tuning.extend_time.max(1.0e-3)).min(1.0);
        self.velocity = self
            .velocity
            .add(wanted.subtract(self.velocity).mul_scalar(respond));
        let hips = self.motion.hips.add(self.velocity.mul_scalar(dt));

        // The dive's own commitment: monotonic, so it never un-stretches, and
        // never restarted by a correction either.
        let extend = (self.motion.extend + dt / tuning.extend_time.max(1.0e-3)).min(1.0);
        // Bank, height and the hand target all EASE toward what the read asks
        // for. A correction that snapped them would put the stutter back in the
        // arms after taking it out of the legs.
        let ease = |from: f32, to: f32| from + (to - from) * respond;
        KeeperMotion {
            hips: Vec3::new(hips.x, hips.y.max(0.20), hips.z),
            lean: ease(self.motion.lean, read.lean),
            extend,
            height_bias: ease(self.motion.height_bias, read.height_bias),
            hands: self
                .motion
                .hands
                .add(self.hand_target(read).subtract(self.motion.hands).mul_scalar(respond)),
        }
    }

    /// Where the hands are being thrown: at the point the keeper believes the
    /// ball will cross, a little in front of the line — `+Z` is toward the
    /// kicker — so the arms reach *out at the ball* rather than sideways along
    /// the line, and meet it a fraction before it crosses.
    ///
    /// This is the read, never the ball. A keeper that could aim its hands at the
    /// real ball would save everything it could physically get near, and the
    /// entire reason a shaped shot beats a keeper is that its read is wrong.
    fn hand_target(&self, read: KeeperRead) -> Vec3 {
        Vec3::new(read.predicted.x, read.predicted.y, self.home.z + 0.34)
    }
}

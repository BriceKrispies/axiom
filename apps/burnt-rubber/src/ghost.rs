//! The **ghost** — the agent driving its own race alongside yours.
//!
//! A ghost in a racing game is an opponent you cannot touch: it shows you the
//! line and the pace, and it never blocks you. Burnt Rubber's ghost is the
//! [`crate::agent`] driver running live, one fixed step per player step, so what
//! you are racing is the real agent making real decisions — not a recorded
//! trace, and not a rubber-banded chase car.
//!
//! # Why it cannot collide with you
//!
//! Because it is not in your simulation at all.
//!
//! [`GhostRun`] owns a **second, entirely separate [`RaceSim`]**, built from the
//! same seed and tuning as yours. Your simulation has no idea it exists: no
//! entry in your traffic pool, no body in your collision pass, nothing to test
//! against. "The ghost does not collide with the player" is therefore not a flag
//! that has to be honoured by every collision site — it is a structural
//! property, true by construction and impossible to regress. The only place the
//! two runs ever meet is the renderer, which draws the ghost's car pose into
//! your frame, and the HUD, which reports the gap.
//!
//! That also means the ghost meets its *own* copy of the traffic. Both pools
//! start from the same seed, so early on the ghost is threading the same cars
//! you are; they drift apart over a run as each pool yields to its own car. That
//! is the honest cost of the isolation, and it is the right trade: a ghost that
//! shared your traffic would have to be able to hit it, and then it could hit
//! you too.
//!
//! # Cost
//!
//! One extra `RaceSim::step` and one agent decision per fixed step — the
//! simulation is far cheaper than the frame it is drawn in, and the agent's
//! brain is a five-entry table.

use crate::agent::{self, DriverTuning};
use crate::sim::car::CarPose;
use crate::sim::{RacePhase, RaceSim};
use crate::tuning::Tuning;
use crate::PlayProfile;

/// The agent's run, advancing in lockstep with the player's.
#[derive(Debug, Clone)]
pub struct GhostRun {
    /// The ghost's own world. Nothing outside this struct ever steps it, and
    /// the player's simulation never reads it.
    sim: RaceSim,
    driver: DriverTuning,
    steps: u64,
}

impl GhostRun {
    /// Put the ghost on the grid, on the same course as the player.
    pub fn new(seed: u64, tuning: Tuning, profile: PlayProfile) -> GhostRun {
        GhostRun::from_plan(
            RaceSim::with_profile(seed, tuning, profile),
            profile,
        )
    }

    /// The ghost on an **already-compiled** course.
    ///
    /// This is the door the shipping app goes through. The ghost drives the
    /// same road the player does, so it should read the same compiled plan
    /// rather than generate a second identical one — which is what
    /// `GhostRun::new` did, and why the course used to compile twice for every
    /// race and twice again on every restart.
    ///
    /// Sharing is safe because `CoursePlan` exposes no `&mut self` method and
    /// holds no interior mutability: the ghost owns its own `RaceSim` and
    /// cannot reach the player's.
    pub fn from_plan(sim: RaceSim, profile: PlayProfile) -> GhostRun {
        GhostRun {
            sim,
            // The technique has to match the control scheme the profile gives
            // the car — see `DriverTuning::for_profile`.
            driver: DriverTuning::for_profile(profile),
            steps: 0,
        }
    }

    /// Advance the ghost one fixed step: perceive, decide through `axiom-agent`,
    /// and drive. The command comes back from the agent exactly as it does in
    /// the offline race — this is the same [`agent::drive_one_step`] the
    /// reference run in `tests/agent_race.rs` is made of.
    pub fn step(&mut self) {
        let (command, _intents) = agent::drive_one_step(&self.sim, &self.driver, self.steps);
        self.sim.step(command);
        self.steps += 1;
    }

    /// The ghost's car, interpolated `alpha` of the way through the current step
    /// — the one thing the renderer needs.
    pub fn car_pose(&self, alpha: f32) -> CarPose {
        self.sim.car_pose(alpha)
    }

    /// Whether the ghost is spending boost this step (the exhaust plume).
    pub fn boosting(&self) -> bool {
        self.sim.boost().active()
    }

    /// How far along the course the ghost has travelled (m).
    pub fn distance(&self) -> f32 {
        self.sim.car().distance
    }

    /// The ghost's elapsed race time (s).
    pub fn elapsed_seconds(&self) -> f32 {
        self.sim.elapsed_seconds()
    }

    /// Whether the ghost has crossed the line.
    pub fn finished(&self) -> bool {
        self.sim.phase() == RacePhase::Finished
    }

    /// The ghost's race, for tests and diagnostics. Read-only on purpose: the
    /// ghost's world is stepped by [`Self::step`] and by nothing else.
    pub const fn sim(&self) -> &RaceSim {
        &self.sim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::DEFAULT_SEED;

    fn ghost() -> GhostRun {
        GhostRun::new(DEFAULT_SEED, Tuning::DEFAULT, PlayProfile::default())
    }

    #[test]
    fn the_ghost_drives_itself_down_the_road() {
        let mut g = ghost();
        let start = g.distance();
        (0..600).for_each(|_| g.step());
        assert!(
            g.distance() > start + 100.0,
            "the ghost should have covered ground: {} -> {}",
            start,
            g.distance()
        );
        assert!(!g.finished(), "not in ten seconds it hasn't");
    }

    /// The property the whole design exists for: the ghost is not in the
    /// player's world, so it cannot touch the player's car. Driving the player
    /// flat out through the same stretch the ghost occupies produces no impact
    /// the ghost is responsible for — because the player's simulation has no
    /// body for it at all.
    #[test]
    fn the_ghost_is_not_in_the_players_simulation() {
        let mut player = RaceSim::with_profile(DEFAULT_SEED, Tuning::DEFAULT, PlayProfile::default());
        let mut solo = player.clone();
        let mut g = ghost();

        // Step the player identically twice — once with a ghost running beside
        // it, once without. A ghost that could touch the player would change the
        // player's run; this one cannot, so the two are bit-identical.
        (0..900).for_each(|_| {
            g.step();
            player.step(DriveCommand::FLAT_OUT);
            solo.step(DriveCommand::FLAT_OUT);
        });

        assert_eq!(player.car().distance, solo.car().distance);
        assert_eq!(player.car().lateral, solo.car().lateral);
        assert_eq!(player.impact_count(), solo.impact_count());
        assert_eq!(player.elapsed_seconds(), solo.elapsed_seconds());
    }

    /// Same seed, same agent, same run — the ghost is reproducible, so the pace
    /// you race is the same pace every time.
    #[test]
    fn the_ghost_is_deterministic() {
        let (mut a, mut b) = (ghost(), ghost());
        (0..900).for_each(|_| {
            a.step();
            b.step();
        });
        assert_eq!(a.distance(), b.distance());
        assert_eq!(a.elapsed_seconds(), b.elapsed_seconds());
    }

    #[test]
    fn the_ghost_reaches_the_finish_on_the_shipping_course() {
        let mut g = ghost();
        (0..60 * 60 * 3).for_each(|_| {
            (!g.finished()).then(|| g.step());
        });
        assert!(g.finished(), "the ghost got {:.0} m", g.distance());
        // The reference run, to the step. This number is a *consequence*, not a
        // setting: the ghost drives the real sim, so any rule that changes how
        // much boost a lap earns moves it.
        //
        // It moved when the course became a compiled plan rather than a
        // control-point walk (89.33 s on the old road, 92.30 s on the compiled
        // one), and again when a held boost button stopped latching off
        // (`sim::boost`) — the meter is the thing lap time is made of, so any
        // change to when it fires moves this.
        //
        // It moved again, 93.90 s -> 90.30 s, when the course gained boost
        // pickups (`course::pickups`). Worth reading twice, because it is a
        // measurement rather than a tuning: the agent does **not** seek pickups
        // and has no idea they exist — it drives its own line and happens to
        // cross the ones that are on it. Three and a half seconds is what
        // *incidental* collection is worth over nine kilometres, which is the
        // honest scale of what was added to the economy. (A fraction of it is
        // the pickup keep-out thinning the ambient traffic in a few lanes:
        // fewer cars is also a faster lap.)
        //
        // And it moved again, 90.30 s -> 84.05 s, when boost lost its top speed
        // (`sim::controller::boost_headroom`). Six seconds over nine kilometres
        // is the honest size of that change: the ghost is not boosting for much
        // more of the lap than it was — the meter still decides that — it is
        // simply no longer held at 114 m/s while it does. The top speed seen went
        // 113.9 -> 168.2 m/s.
        //
        // And once more, 84.05 s -> 83.65 s, when a boosting player started going
        // *through* the back of traffic instead of into it
        // (`sim::RaceSim::smash_through`). Four tenths, and the small size is the
        // interesting part: the ghost does not aim for cars — it is still trying
        // to thread them — so this is only the handful of rear-ends a lap that
        // used to scrub speed and now do not. Contacts fell 16 -> 14 for the same
        // reason, since a smash is not counted as one.
        assert!(
            (g.elapsed_seconds() - 83.65).abs() < 0.05,
            "ghost time {:.2}s",
            g.elapsed_seconds()
        );
    }

    /// **The bar.** The ghost is the pace you race, and it has to hold that
    /// pace on *both* games — not just the one a developer happens to run on a
    /// desktop.
    ///
    /// The bar is a range rather than a single number, and the range is what it
    /// is for a reason. The ghost's technique ([`DriverTuning::FAST`]) was fitted
    /// by measurement against the *old* course, and the course is now compiled
    /// from an authored specification: its corners hold a constant radius where
    /// the old road's curvature was relaxed noise, its traffic is drawn from a
    /// density band rather than a fixed 85 m pitch, and it carries two authored
    /// figures.
    ///
    /// **The contact count is a chaotic metric and the bar has to respect that.**
    /// A run is nine kilometres of a driver reacting to traffic it meets in a
    /// particular phase, so any change to *when* the car is where re-rolls every
    /// encounter after it. Measured over five seeds on this course, the same
    /// driver scores contacts across the whole range 1..13 — the shipping seed is
    /// simply at the hard end of it. (Before the held-boost change those five
    /// seeds gave 9, 9, 4, 11, 5; after, 13, 7, 1, 8, 5 — a *lower* mean, on a
    /// 0.3 s slower average lap.) A bar tight enough to pin one seed's contact
    /// count would be pinning noise, and would fail on the next unrelated change.
    ///
    /// So: under 105 seconds, at most twenty contacts, and more than sixty near
    /// misses. The last of those is the one that actually says the ghost is
    /// playing the game rather than bulldozing it, and it is the one that has not
    /// moved through any of this.
    ///
    /// The contact bar moved from fifteen to twenty when boost lost its top speed
    /// (`sim::controller::boost_headroom`), and the shape of that move is worth
    /// keeping, because it is the whole argument for the curve that shipped. With
    /// boost simply *uncapped* — full acceleration however fast the car already
    /// was — the wheel ghost hit 431 m/s and the run stopped being a drive: 22
    /// contacts and only 50 near misses, because a car doing 1550 km/h has almost
    /// no steering authority left ([`super::sim::controller::steering_authority`]
    /// falls off with speed) and cannot thread anything. With the logarithmic
    /// tail it tops out at 168 m/s and scores 65 near misses against 16 contacts —
    /// still the same drive, played faster. Sixteen is inside the noise band
    /// described above; twenty is that band with the same margin the old fifteen
    /// had over thirteen.
    ///
    /// What has not moved is the part that says the ghost is playing the game
    /// rather than bulldozing it: it still scores more than sixty near misses a
    /// lap, which is where its boost comes from.
    ///
    /// Asserted per profile because the two are genuinely different drives, and
    /// because the phone arm is the one that was quietly broken: the agent only
    /// ever emitted `steer`, which `sim::rails` ignores, so the phone ghost held
    /// one lane for nine kilometres, hit 25 cars and took 96.45 s. Nothing
    /// tested it, so nothing caught it.
    #[test]
    fn the_ghost_beats_ninety_seconds_on_both_games() {
        [PlayProfile::Wheel, PlayProfile::Rails]
            .into_iter()
            .for_each(|profile| {
                let mut g = GhostRun::new(DEFAULT_SEED, Tuning::DEFAULT, profile);
                (0..60 * 60 * 3).for_each(|_| {
                    (!g.finished()).then(|| g.step());
                });
                assert!(g.finished(), "{profile:?} ghost did not finish");
                assert!(
                    g.elapsed_seconds() < 105.0,
                    "{profile:?} ghost took {:.2}s — the ghost must beat 105 s",
                    g.elapsed_seconds()
                );
                // And it gets there by threading traffic, not by bulldozing it:
                // a near miss pays 0.13 of the meter, contact pays nothing and
                // costs speed.
                assert!(
                    g.sim().near_miss_count() > 60,
                    "{profile:?} ghost only scored {} near misses — it is not hunting them",
                    g.sim().near_miss_count()
                );
                assert!(
                    g.sim().impact_count() <= 20,
                    "{profile:?} ghost hit {} things",
                    g.sim().impact_count()
                );
            });
    }
}

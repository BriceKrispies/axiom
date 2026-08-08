//! The shot matrix: every shape of shot the game can produce, played headlessly
//! against the keeper, and counted.
//!
//! This is the game's own instrument for the one question no amount of reading
//! the code answers: **how often does the keeper actually save it?** It sweeps
//! the whole authorable space — every corner, every bend, every arc, every place
//! a curve can break — plays each shot against a set of seeded keepers, and
//! reports what happened.
//!
//! It lives in `src` rather than in a test because it is not only a test. It is
//! how the game is tuned: change a keeper number, run the sweep, and see what it
//! did to every shot at once rather than to the three you thought to try. The
//! test suite and the reporting example are both just callers.
//!
//! Every run is deterministic. A shot is `(shape, seed)` and nothing else, so a
//! surprising cell in the report can be reproduced exactly by replaying that one
//! pair.

use crate::pitch::GoalMouth;
use crate::play::{PlayCommand, Phase, Session, ShotResult};
use crate::shot::{BendCurve, GoalTarget, ShotIntent};
use crate::stroke::Pace;
use crate::tuning::Tuning;

mod tally;

pub use tally::{group_by, sweep, sweep_detailed, totals, Outcomes, Row};

/// One shot to take: where it finishes and what shape it takes to get there.
///
/// `bend` and `loft` are fractions of what the game allows, signed, so a matrix
/// is independent of the tuning it is swept against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotSpec {
    pub h: f32,
    pub v: f32,
    pub bend: f32,
    pub bend_at: f32,
    pub loft: f32,
    pub loft_at: f32,
    /// How hard it was hit, `0` a careful stroke to `1` a flick.
    pub pace: f32,
}

impl ShotSpec {
    /// The shot as the session would receive it from a reading.
    pub fn intent(&self, tuning: &Tuning) -> ShotIntent {
        ShotIntent::curved(
            GoalTarget::new(self.h, self.v),
            BendCurve::through(
                self.bend_at,
                self.bend * tuning.bend.max_offset,
                tuning.bend.peak_margin,
            ),
            BendCurve::through(
                self.loft_at,
                self.loft * tuning.loft.max_offset,
                tuning.loft.peak_margin,
            ),
            Pace {
                speed: self.pace,
                // An even hand: the sweep varies how *hard* a shot is hit
                // separately from how it is shaped, and folding the easing in
                // here would confound the two.
                easing: 0.0,
            },
        )
    }

    /// Where this shot finishes, in metres.
    pub fn finish(&self, tuning: &Tuning) -> axiom::prelude::Vec3 {
        GoalMouth::new(tuning.goal.inset).to_world(self.h, self.v)
    }
}

/// The axes the full matrix sweeps.
pub const AIM_ACROSS: [f32; 9] = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0];
pub const AIM_UP: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
pub const BEND: [f32; 7] = [-1.0, -0.6, -0.3, 0.0, 0.3, 0.6, 1.0];
pub const LOFT: [f32; 5] = [-1.0, -0.4, 0.0, 0.4, 1.0];
pub const BREAK_AT: [f32; 3] = [0.3, 0.5, 0.7];
pub const PACE: [f32; 3] = [0.0, 0.5, 1.0];

/// The coarser axes, for the sweep that runs on every `cargo test`.
pub const COARSE_ACROSS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];
pub const COARSE_UP: [f32; 3] = [0.0, 0.5, 1.0];
pub const COARSE_SHAPE: [f32; 3] = [-1.0, 0.0, 1.0];
pub const COARSE_BREAK: [f32; 2] = [0.3, 0.7];
pub const COARSE_PACE: [f32; 2] = [0.15, 0.85];

/// Every shot in the matrix: every corner, every bend, every arc, and every
/// place a curve can break.
pub fn full_matrix() -> Vec<ShotSpec> {
    matrix_over(&AIM_ACROSS, &AIM_UP, &BEND, &LOFT, &BREAK_AT, &PACE)
}

/// A coarser matrix over the same space — the one the default test suite plays,
/// so `cargo test` stays a thing you run without thinking about it.
pub fn coarse_matrix() -> Vec<ShotSpec> {
    matrix_over(
        &COARSE_ACROSS,
        &COARSE_UP,
        &COARSE_SHAPE,
        &COARSE_SHAPE,
        &COARSE_BREAK,
        &COARSE_PACE,
    )
}

/// Build a matrix over explicit axes.
///
/// Where a curve is flat its break point is meaningless, so those combinations
/// are collapsed rather than swept — otherwise a third of the matrix would be
/// the same shot counted several times, and every rate in the report would be
/// quietly weighted toward straight shots.
pub fn matrix_over(
    across: &[f32],
    up: &[f32],
    bends: &[f32],
    lofts: &[f32],
    breaks: &[f32],
    paces: &[f32],
) -> Vec<ShotSpec> {
    let points = |magnitude: f32| -> Vec<f32> {
        match magnitude == 0.0 {
            true => vec![0.5],
            false => breaks.to_vec(),
        }
    };
    across
        .iter()
        .flat_map(|h| up.iter().map(move |v| (*h, *v)))
        .flat_map(|(h, v)| bends.iter().map(move |bend| (h, v, *bend)))
        .flat_map(move |(h, v, bend)| {
            points(bend)
                .into_iter()
                .map(move |bend_at| (h, v, bend, bend_at))
        })
        .flat_map(|(h, v, bend, bend_at)| {
            lofts
                .iter()
                .map(move |loft| (h, v, bend, bend_at, *loft))
        })
        .flat_map(move |(h, v, bend, bend_at, loft)| {
            points(loft)
                .into_iter()
                .map(move |loft_at| (h, v, bend, bend_at, loft, loft_at))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|(h, v, bend, bend_at, loft, loft_at)| {
            paces.to_vec().into_iter().map(move |pace| ShotSpec {
                h,
                v,
                bend,
                bend_at,
                loft,
                loft_at,
                pace,
            })
        })
        .collect()
}

/// A run of keeper seeds. Spread widely, because a seed IS a keeper: each one
/// produces exactly one nerve for a cold attempt, so a handful of seeds is a
/// handful of keepers and aliases badly. Ask for enough of them.
pub fn keepers(count: u64) -> Vec<u64> {
    (0..count)
        .map(|i| 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(i + 1) ^ 0x5EED)
        .collect()
}

/// Take one shot, against the keeper this seed produces, and report the outcome.
///
/// Every attempt is a fresh session, so the keeper's memory of previous shots
/// never leaks between cells — the matrix measures how a shot fares *cold*,
/// which is the only comparable thing to measure.
pub fn take(spec: &ShotSpec, seed: u64, tuning: Tuning) -> ShotResult {
    let mut session = Session::seeded(tuning, seed);
    while session.phase() != Phase::Aiming {
        session.step(&[]);
    }
    session.step(&[PlayCommand::Kick(spec.intent(&tuning))]);
    let mut spent = 0u32;
    while session.result().is_none() && spent < 600 {
        session.step(&[]);
        spent += 1;
    }
    session.result().unwrap_or(ShotResult::Miss)
}

/// Take one shot against the **average** keeper — no jitter, no guess.
///
/// The comparable measurement when the question is about the *pitch* rather than
/// about the dice: two shots that mirror each other must come out the same, and
/// a rolled keeper would hide that behind its own luck.
pub fn take_steady(spec: &ShotSpec, tuning: Tuning) -> ShotResult {
    let mut session = Session::steady(tuning);
    while session.phase() != Phase::Aiming {
        session.step(&[]);
    }
    session.step(&[PlayCommand::Kick(spec.intent(&tuning))]);
    let mut spent = 0u32;
    while session.result().is_none() && spent < 600 {
        session.step(&[]);
        spent += 1;
    }
    session.result().unwrap_or(ShotResult::Miss)
}

impl ShotSpec {
    /// The same shot, mirrored across the centre of the goal.
    pub fn mirrored(&self) -> ShotSpec {
        ShotSpec {
            h: -self.h,
            bend: -self.bend,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_covers_the_whole_authorable_space_without_repeating_itself() {
        let matrix = full_matrix();
        // Every corner, every height, every bend and every arc is in there.
        AIM_ACROSS
            .iter()
            .for_each(|h| assert!(matrix.iter().any(|s| s.h == *h), "missing aim {h}"));
        AIM_UP
            .iter()
            .for_each(|v| assert!(matrix.iter().any(|s| s.v == *v), "missing height {v}"));
        BEND.iter()
            .for_each(|b| assert!(matrix.iter().any(|s| s.bend == *b), "missing bend {b}"));
        LOFT.iter()
            .for_each(|l| assert!(matrix.iter().any(|s| s.loft == *l), "missing loft {l}"));
        BREAK_AT.iter().for_each(|at| {
            assert!(matrix.iter().any(|s| s.bend_at == *at && s.bend != 0.0));
            assert!(matrix.iter().any(|s| s.loft_at == *at && s.loft != 0.0));
        });
        // A flat curve has exactly one break point, not three.
        assert!(matrix
            .iter()
            .filter(|s| s.bend == 0.0)
            .all(|s| s.bend_at == 0.5));
        // Nothing is listed twice.
        PACE.iter()
            .for_each(|p| assert!(matrix.iter().any(|s| s.pace == *p), "missing pace {p}"));
        let mut keys: Vec<String> = matrix
            .iter()
            .map(|s| {
                format!(
                    "{:?}",
                    (s.h, s.v, s.bend, s.bend_at, s.loft, s.loft_at, s.pace)
                )
            })
            .collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "the matrix repeats itself");
    }

    #[test]
    fn a_shot_taken_twice_the_same_way_comes_out_the_same_way() {
        let spec = ShotSpec {
            h: 0.5,
            v: 0.25,
            bend: -0.6,
            bend_at: 0.7,
            loft: 0.4,
            loft_at: 0.5,
            pace: 0.5,
        };
        assert_eq!(
            take(&spec, 11, Tuning::DEFAULT),
            take(&spec, 11, Tuning::DEFAULT)
        );
    }

    #[test]
    fn different_keepers_do_different_things_with_the_same_shot() {
        // Somewhere in a run of seeds, one keeper does something another does not
        // — otherwise the nerve is not reaching the pitch at all.
        let spec = ShotSpec {
            h: 0.2,
            v: 0.9,
            bend: 0.0,
            bend_at: 0.5,
            loft: 0.4,
            loft_at: 0.5,
            pace: 0.5,
        };
        let results: Vec<ShotResult> = (0..24)
            .map(|seed| take(&spec, seed, Tuning::DEFAULT))
            .collect();
        assert!(
            results.iter().any(|r| *r != results[0]),
            "every seeded keeper produced {:?}",
            results[0]
        );
    }

    #[test]
    fn outcomes_count_what_happened() {
        let mut out = Outcomes::default();
        out.record(ShotResult::Goal);
        out.record(ShotResult::Goal);
        out.record(ShotResult::Save);
        out.record(ShotResult::Miss);
        assert_eq!(out.total(), 4);
        assert!((out.goal_rate() - 0.5).abs() < 1.0e-6);
        assert!((out.save_rate() - 0.25).abs() < 1.0e-6);
        let mut other = Outcomes::default();
        other.record(ShotResult::Frame(crate::pitch::FrameMember::LeftPost));
        out.merge(&other);
        assert_eq!(out.total(), 5);
        assert_eq!(out.frame, 1);
        assert_eq!(Outcomes::default().save_rate(), 0.0);
    }

    #[test]
    fn a_small_sweep_groups_and_totals_correctly() {
        let specs: Vec<ShotSpec> = [-1.0f32, 1.0]
            .iter()
            .map(|h| ShotSpec {
                h: *h,
                v: 0.5,
                bend: 0.0,
                bend_at: 0.5,
                loft: 0.4,
                loft_at: 0.5,
                pace: 0.5,
            })
            .collect();
        let seeds = [1u64, 2, 3];
        let total = sweep(&specs, &seeds, Tuning::DEFAULT);
        assert_eq!(total.total(), 6);
        // The detailed sweep sees the same shots, and grouping preserves them.
        let detailed = sweep_detailed(&specs, &seeds, Tuning::DEFAULT);
        assert_eq!(detailed.len(), 6);
        assert_eq!(totals(&detailed), total);
        let rows = group_by(&detailed, |s| format!("h {:+.2}", s.h));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().map(|(_, o)| o.total()).sum::<u32>(), 6);
    }
}

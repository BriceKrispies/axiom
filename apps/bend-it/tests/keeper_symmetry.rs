//! The pitch is symmetric, and the keeper is too.
//!
//! Nothing in this game should care which side of the goal a shot goes to. The
//! kicker stands to one side and runs up at an angle, the shot's own right-hand
//! axis tilts with its aim, and the keeper's dive banks — plenty of places for a
//! sign to go astray and produce a game that is quietly easier to score on one
//! side than the other. A player would feel that long before they could name it.
//!
//! Measured against the **average** keeper: a rolled one would hide an asymmetry
//! behind its own luck.

use axiom_bend_it::matrix::{full_matrix, take_steady, ShotSpec};
use axiom_bend_it::play::ShotResult;
use axiom_bend_it::tuning::Tuning;

#[test]
fn a_shot_and_its_mirror_image_come_out_the_same() {
    let tuning = Tuning::DEFAULT;
    let mismatches: Vec<(ShotSpec, ShotResult, ShotResult)> = full_matrix()
        .into_iter()
        .filter(|spec| spec.h > 0.0)
        .map(|spec| {
            (
                spec,
                take_steady(&spec, tuning),
                take_steady(&spec.mirrored(), tuning),
            )
        })
        .filter(|(_, a, b)| a != b)
        .collect();
    let total = full_matrix().iter().filter(|s| s.h > 0.0).count();
    assert!(
        mismatches.is_empty(),
        "{} of {total} mirrored shots disagree; first few:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(6)
            .map(|(s, a, b)| format!(
                "  h {:+.2} v {:.2} bend {:+.2}@{:.2} loft {:+.2}@{:.2}: {a:?} vs mirrored {b:?}",
                s.h, s.v, s.bend, s.bend_at, s.loft, s.loft_at
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

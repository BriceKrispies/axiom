//! Counting a sweep, and slicing it.
//!
//! Split from [`super`] because building the shot list and taking the shots is a
//! different job from tallying what came back — and the tally is the part a
//! report re-asks a dozen different questions of.

use crate::play::ShotResult;
use crate::tuning::Tuning;

use super::{take, ShotSpec};

/// What a set of shots came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcomes {
    pub goals: u32,
    pub saves: u32,
    pub frame: u32,
    pub misses: u32,
}

impl Outcomes {
    pub fn record(&mut self, result: ShotResult) {
        match result {
            ShotResult::Goal => self.goals += 1,
            ShotResult::Save => self.saves += 1,
            ShotResult::Frame(_) => self.frame += 1,
            ShotResult::Miss => self.misses += 1,
        }
    }

    pub fn total(&self) -> u32 {
        self.goals + self.saves + self.frame + self.misses
    }

    /// The share of shots the keeper stopped, `0..1`.
    pub fn save_rate(&self) -> f32 {
        self.saves as f32 / self.total().max(1) as f32
    }

    /// The share that went in, `0..1`.
    pub fn goal_rate(&self) -> f32 {
        self.goals as f32 / self.total().max(1) as f32
    }

    pub fn merge(&mut self, other: &Outcomes) {
        self.goals += other.goals;
        self.saves += other.saves;
        self.frame += other.frame;
        self.misses += other.misses;
    }
}

/// One row of a breakdown: a label and what happened under it.
pub type Row = (String, Outcomes);

/// Sweep a matrix against a set of keeper seeds.
pub fn sweep(specs: &[ShotSpec], seeds: &[u64], tuning: Tuning) -> Outcomes {
    specs
        .iter()
        .flat_map(|spec| seeds.iter().map(move |seed| (spec, *seed)))
        .fold(Outcomes::default(), |mut out, (spec, seed)| {
            out.record(take(spec, seed, tuning));
            out
        })
}

/// Sweep once, keeping every result.
///
/// Every breakdown a report wants is a different grouping of the *same* shots,
/// so the sweep is run once and grouped many times. Re-running it per breakdown
/// would multiply the cost of the report by the number of questions asked of it.
pub fn sweep_detailed(specs: &[ShotSpec], seeds: &[u64], tuning: Tuning) -> Vec<(ShotSpec, ShotResult)> {
    specs
        .iter()
        .flat_map(|spec| seeds.iter().map(move |seed| (spec, *seed)))
        .map(|(spec, seed)| (*spec, take(spec, seed, tuning)))
        .collect()
}

/// Group finished results by a label derived from each shot.
pub fn group_by(
    results: &[(ShotSpec, ShotResult)],
    label: impl Fn(&ShotSpec) -> String,
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    results.iter().for_each(|(spec, result)| {
        let key = label(spec);
        match rows.iter_mut().find(|(name, _)| *name == key) {
            Some((_, cell)) => cell.record(*result),
            None => {
                let mut cell = Outcomes::default();
                cell.record(*result);
                rows.push((key, cell));
            }
        }
    });
    rows
}

/// The totals of a finished sweep.
pub fn totals(results: &[(ShotSpec, ShotResult)]) -> Outcomes {
    results.iter().fold(Outcomes::default(), |mut out, (_, r)| {
        out.record(*r);
        out
    })
}


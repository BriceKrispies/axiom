//! Sweep every shot the game can produce against seeded keepers, and report how
//! often the keeper saves it.
//!
//! ```sh
//! cargo run --release -p axiom-bend-it --example keeper_report -- [seeds]
//! ```
//!
//! Every run is deterministic: a cell is `(shape, seed)` and nothing else, so any
//! surprising number can be reproduced exactly.

use std::time::Instant;

use axiom_bend_it::matrix::{
    full_matrix, group_by, keepers, sweep_detailed, totals, Outcomes, Row, ShotSpec,
};
use axiom_bend_it::play::ShotResult;
use axiom_bend_it::tuning::Tuning;

fn main() {
    let seeds: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(8);
    let seeds = keepers(seeds);
    let matrix = full_matrix();
    let shots = matrix.len() as u64 * seeds.len() as u64;
    println!(
        "BEND IT — keeper sweep\n{} shapes x {} keepers = {} penalties\n",
        matrix.len(),
        seeds.len(),
        shots
    );

    let started = Instant::now();
    let results = sweep_detailed(&matrix, &seeds, Tuning::DEFAULT);
    let elapsed = started.elapsed();
    let total = totals(&results);

    report("BY AIM ACROSS", &results, |s| num(s.h));
    report("BY AIM HEIGHT", &results, |s| num(s.v));
    report("BY BEND", &results, |s| num(s.bend));
    report("BY ARC", &results, |s| num(s.loft));
    report("BY PACE (how fast it was drawn)", &results, |s| num(s.pace));
    report("BY WHERE THE BEND BREAKS", &results, |s| match s.bend == 0.0 {
        true => "straight".into(),
        false => num(s.bend_at),
    });
    report("BY WHERE THE ARC PEAKS", &results, |s| match s.loft == 0.0 {
        true => "flat".into(),
        false => num(s.loft_at),
    });
    report("BY CORNER", &results, |s| format!("{} {}", band(s.v - 0.5, 0.2, ["low", "middle", "high"]), band(s.h, 0.4, ["left", "middle", "right"])));

    println!(
        "\n{} penalties in {:.1}s  ({:.0} shots/s)\nKEEPER SAVED {:.1}%   scored {:.1}%   frame {:.1}%   wide {:.1}%",
        total.total(),
        elapsed.as_secs_f32(),
        total.total() as f32 / elapsed.as_secs_f32().max(1.0e-3),
        total.save_rate() * 100.0,
        total.goal_rate() * 100.0,
        total.frame as f32 / total.total().max(1) as f32 * 100.0,
        total.misses as f32 / total.total().max(1) as f32 * 100.0,
    );
}

/// Print one breakdown, and return its total.
fn report(
    title: &str,
    results: &[(ShotSpec, ShotResult)],
    label: impl Fn(&ShotSpec) -> String,
) -> Outcomes {
    let mut rows: Vec<Row> = group_by(results, label);
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    println!("{title}");
    println!("  {:<12} {:>8} {:>8} {:>8} {:>8}", "", "shots", "saved", "goal", "frame");
    let mut total = Outcomes::default();
    rows.iter().for_each(|(name, out)| {
        total.merge(out);
        println!(
            "  {:<12} {:>8} {:>7.1}% {:>7.1}% {:>7.1}%",
            name,
            out.total(),
            out.save_rate() * 100.0,
            out.goal_rate() * 100.0,
            out.frame as f32 / out.total().max(1) as f32 * 100.0,
        );
    });
    println!();
    total
}

/// A stable label for one of the matrix's numeric axes.
fn num(value: f32) -> String {
    format!("{value:+.2}")
}

/// Which of three bands a signed value falls in.
fn band(value: f32, edge: f32, names: [&str; 3]) -> &str {
    match (value < -edge, value > edge) {
        (true, _) => names[0],
        (_, true) => names[2],
        _ => names[1],
    }
}

//! Watch the agent take a set of penalties, headlessly.
//!
//! ```sh
//! cargo run -p axiom-bend-it --example playthrough -- [attempts] [seed]
//! ```
//!
//! Every line is one attempt: what the striker aimed at, the shape it authored,
//! where the keeper went, and what happened. It is deterministic, so a run is a
//! reproducible measurement of the agent *and* of the game's balance.

use axiom_bend_it::agent::Striker;
use axiom_bend_it::play::{Phase, Session};
use axiom_bend_it::tuning::Tuning;

/// A balance sweep: how often each *shape* of shot beats the keeper, over a grid
/// of targets. Run with `-- sweep`.
fn sweep() {
    use axiom_bend_it::play::{EditorCommand, ShotResult};
    use axiom_bend_it::shot::{BendCurve, GoalTarget};
    let play = |h: f32, v: f32, bend: f32, bend_at: f32, loft: f32, loft_at: f32| {
        let mut s = Session::new(Tuning::DEFAULT);
        (0..12).for_each(|_| s.step(&[]));
        s.step(&[
            EditorCommand::Aim(GoalTarget::new(h, v)),
            EditorCommand::Advance,
            EditorCommand::SetBend(BendCurve::through(bend_at, bend, 0.14)),
            EditorCommand::Advance,
            EditorCommand::SetLoft(BendCurve::through(loft_at, loft, 0.14)),
            EditorCommand::Advance,
        ]);
        let mut n = 0;
        while s.result().is_none() && n < 900 {
            s.step(&[]);
            n += 1;
        }
        s.result()
    };
    let shapes: [(&str, f32, f32, f32, f32); 9] = [
        ("straight flat  ", 0.0, 0.5, 0.0, 0.5),
        ("straight normal", 0.0, 0.5, 0.9, 0.5),
        ("bend early     ", 2.0, 0.28, 0.9, 0.5),
        ("bend mid       ", 2.0, 0.50, 0.9, 0.5),
        ("bend late      ", 2.0, 0.74, 0.9, 0.5),
        ("bend half late ", 1.0, 0.74, 0.9, 0.5),
        ("loft late      ", 0.0, 0.5, 2.6, 0.74),
        ("loft early     ", 0.0, 0.5, 2.6, 0.28),
        ("dip            ", 0.0, 0.5, -1.3, 0.5),
    ];
    for (name, bend, bend_at, loft, loft_at) in shapes {
        let mut goals = 0;
        let mut total = 0;
        let mut grid = String::new();
        for v in [0.9f32, 0.6, 0.3, 0.05] {
            for h in [-0.95f32, -0.7, -0.45, -0.2, 0.0, 0.2, 0.45, 0.7, 0.95] {
                let r = play(h, v, -h.signum() * bend, bend_at, loft, loft_at);
                total += 1;
                goals += u32::from(matches!(r, Some(ShotResult::Goal)));
                grid.push(match r {
                    Some(ShotResult::Goal) => 'O',
                    Some(ShotResult::Save) => '.',
                    Some(ShotResult::Frame(_)) => '#',
                    _ => '?',
                });
            }
            grid.push('\n');
        }
        println!(
            "{name}  goals {:.0}%\n{grid}",
            100.0 * goals as f32 / total as f32
        );
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    if std::env::args().any(|a| a == "sweep") {
        sweep();
        return;
    }
    let attempts: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(10);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1);

    let mut session = Session::new(Tuning::DEFAULT);
    let mut striker = Striker::new(seed);
    let mut reported = 0u32;
    let mut ticks = 0u32;

    println!("BEND IT — agent play-through (seed {seed})");
    println!(
        "{:>3}  {:^13}  {:^24}  {:^15}  {}",
        "#", "aim", "shape", "keeper", "result"
    );
    while (reported < attempts) & (ticks < attempts * 400) {
        let was = session.phase();
        striker.play(&mut session);
        ticks += 1;
        // Report on the tick the attempt resolves.
        let resolved = (was != Phase::Resolution) & (session.phase() == Phase::Resolution);
        if resolved {
            reported += 1;
            let intent = session.intent();
            let (bend_at, bend) = intent.bend.peak();
            let (loft_at, loft) = intent.loft.peak();
            let keeper = session
                .keeper()
                .read()
                .map(|r| format!("dived {:+.2} m", r.aim.x))
                .unwrap_or_else(|| "stayed".into());
            println!(
                "{:>3}  h{:+.2} v{:.2}  bend {:+.2}@{:.2} lift {:+.2}@{:.2}  {:^15}  {}",
                reported,
                intent.target.h,
                intent.target.v,
                bend,
                bend_at,
                loft,
                loft_at,
                keeper,
                session
                    .result()
                    .map(|r| r.banner())
                    .unwrap_or("?")
            );
        }
    }
    let tally = session.tally();
    println!(
        "\nFINAL  {} / {} scored  ({:.0}%)",
        tally.goals,
        tally.attempts,
        100.0 * tally.goals as f32 / tally.attempts.max(1) as f32
    );
}

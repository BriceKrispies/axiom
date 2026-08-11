//! Measures the **only** procedural generation Burnt Rubber still does while
//! the race is running: `props_for_chunk`, called from `SceneryField::refresh`
//! each time the sliding chunk window advances.
//!
//! Everything else — the course compile, the three albedos, all 24 road spans
//! and all ~927 paint chunks — is built before the first frame, and was already
//! built before the first frame prior to the preparation phase. This is the one
//! generator whose cost lands inside a frame, so it is the one whose cost
//! decides whether pre-baking scenery would buy anything.
//!
//! ```text
//! cargo run --release --example scenery_cost -p axiom-burnt-rubber
//! ```

use std::time::Instant;

use axiom_burnt_rubber::render::scenery::{props_for_chunk, PropInstance, SCENERY_CHUNK_LENGTH};
use axiom_burnt_rubber::{Tuning, DEFAULT_SEED};

fn main() {
    let plan = axiom_burnt_rubber::course::procedural::plan_for(DEFAULT_SEED, &Tuning::DEFAULT)
        .expect("the shipping course compiles");
    let track = plan.track();
    let tuning = Tuning::DEFAULT.course;
    let chunks = (plan.length() / SCENERY_CHUNK_LENGTH).ceil() as usize;

    let mut scratch: Vec<PropInstance> = Vec::new();

    // Warm-up: the first chunk pays for cold pages and a growing scratch buffer.
    (0..8).for_each(|index| props_for_chunk(DEFAULT_SEED, track, index, &tuning, &mut scratch));

    // Per-chunk cost, over the whole course. Each figure is what one advance of
    // the sliding window costs, because an advance admits exactly one new chunk.
    let mut per_chunk_us: Vec<f64> = Vec::with_capacity(chunks);
    let mut props_total = 0usize;
    (0..chunks).for_each(|index| {
        let start = Instant::now();
        props_for_chunk(DEFAULT_SEED, track, index, &tuning, &mut scratch);
        per_chunk_us.push(start.elapsed().as_secs_f64() * 1.0e6);
        props_total += scratch.len();
    });

    // Whole-course cost: what pre-baking every chunk at startup would add to
    // launch, and equally the total the race currently pays spread out.
    let start = Instant::now();
    let baked: usize = (0..chunks)
        .map(|index| {
            let mut out = Vec::new();
            props_for_chunk(DEFAULT_SEED, track, index, &tuning, &mut out);
            out.len()
        })
        .sum();
    let bake_ms = start.elapsed().as_secs_f64() * 1000.0;

    let mut sorted = per_chunk_us.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN timings"));
    let median = sorted[sorted.len() / 2];
    let worst = sorted[sorted.len() - 1];
    let total_us: f64 = per_chunk_us.iter().sum();

    println!("course            {:.0} m, {chunks} scenery chunks", plan.length());
    println!("props             {props_total} instances over the course");
    println!();
    println!("ONE window advance (the in-race cost, ~once/second at speed):");
    println!("  median          {median:.1} us  ({:.4} ms)", median / 1000.0);
    println!("  worst           {worst:.1} us  ({:.4} ms)", worst / 1000.0);
    println!("  as % of a 16.7 ms frame: {:.3}%", worst / 1000.0 / 16.667 * 100.0);
    println!();
    println!("WHOLE COURSE pre-baked at startup (what moving it would cost launch):");
    println!("  generate all    {bake_ms:.2} ms, {baked} props (with per-chunk allocation)");
    println!("  sum of parts    {:.2} ms", total_us / 1000.0);
}

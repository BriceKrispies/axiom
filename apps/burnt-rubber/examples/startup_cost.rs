//! Measures the cost of one Burnt Rubber launch: `BurntRubber::with_profile`
//! from call to playable, which is the whole startup path — course compile,
//! albedo synthesis, road cut, scene install.
//!
//! Deliberately calls nothing but the public constructor, so the *identical*
//! file compiles against the pre-preparation tree and the post-preparation tree
//! and the two numbers are comparable.
//!
//! ```text
//! cargo run --release --example startup_cost -p axiom-burnt-rubber -- <reps>
//! ```
//!
//! Prints one millisecond figure per line so a caller can pool runs. Interleave
//! the two builds rather than running one then the other: this machine drifts
//! enough between processes that a block of A followed by a block of B measures
//! the machine warming up as much as it measures the code.

use std::time::Instant;

use axiom_burnt_rubber::{BurntRubber, PlayProfile, Tuning, DEFAULT_SEED, HEIGHT, WIDTH};

fn launch() -> BurntRubber {
    BurntRubber::with_profile(DEFAULT_SEED, Tuning::DEFAULT, WIDTH, HEIGHT, PlayProfile::Wheel)
}

fn main() {
    let reps: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(10);

    // Two discarded launches. The first touches cold pages and first-call
    // branch predictors; measuring it would report the allocator's warm-up as
    // if it were the course generator's cost.
    (0..2).for_each(|_| {
        std::hint::black_box(launch());
    });

    (0..reps).for_each(|_| {
        let start = Instant::now();
        let app = std::hint::black_box(launch());
        let elapsed = start.elapsed();
        // Dropped *after* the clock stops: teardown is not launch cost, but the
        // value has to outlive the measurement or the optimiser is free to
        // sink the whole construction past it.
        drop(app);
        println!("{:.2}", elapsed.as_secs_f64() * 1000.0);
    });
}

// Path contains `modules/.../src`, so this is engine code. The `#[sim]` markers
// are written exactly as `axiom_zones::sim` injects them, so the fixture needs no
// dependency on the markers crate.
#![allow(dead_code, non_upper_case_globals)]

use std::time::{Instant, SystemTime};

// A `#[sim]` zone: time reads here are FLAGGED.
fn sim_step() {
    const __engine_zone_sim: () = ();
    let _ = Instant::now();
    let _ = SystemTime::now();
}

// Not a sim zone: the same calls are fine.
fn plain_helper() {
    let _ = Instant::now();
    let _ = SystemTime::now();
}

// A `#[sim]` zone with no time read: nothing flagged.
fn sim_pure() {
    const __engine_zone_sim: () = ();
    let _ = 1 + 1;
}

// A `#[sim]` MODULE: a time read anywhere inside is flagged, even from a function
// that isn't individually marked.
mod sim_zone {
    const __engine_zone_sim: () = ();

    pub fn inner() {
        let _ = std::time::Instant::now();
    }
}

fn main() {}

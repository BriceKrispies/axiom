// Path contains `apps/`, not `crates/` or `modules/`, so it is a composition
// leaf outside the engine spine: the lint must NOT fire, even on a `#[sim]`-marked
// fused multiply-add. (Expected output: empty.)
#![allow(dead_code, non_upper_case_globals)]

fn sim_step(v: f32, dt: f32, p: f32) -> f32 {
    const __engine_zone_sim: () = ();
    v.mul_add(dt, p)
}

fn main() {}

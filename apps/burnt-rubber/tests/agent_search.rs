//! A multi-start random search over driving technique. Ignored by default —
//! a measuring tool, not a gate.
//!
//! Coordinate descent converged and then refused to move on any single axis,
//! four separate times, which is the signature of a local optimum rather than a
//! limit. This perturbs every knob at once from several starts, which is the
//! cheapest way to find out which of the two it was.
use axiom_burnt_rubber::agent::{self, DriverTuning};
use axiom_burnt_rubber::tuning::Tuning;
use axiom_burnt_rubber::{PlayProfile, RaceSim};

fn sim_for(profile: PlayProfile) -> RaceSim {
    RaceSim::with_profile(axiom_burnt_rubber::DEFAULT_SEED, Tuning::DEFAULT, profile)
}

/// A deterministic LCG. `Math.random` equivalents are banned in this repo's
/// tooling for good reason: a search whose result cannot be reproduced is an
/// anecdote.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as f32) / ((1u64 << 31) as f32)
    }
    /// A multiplicative jitter around `v`, clamped to `[lo, hi]`.
    fn jitter(&mut self, v: f32, spread: f32, lo: f32, hi: f32) -> f32 {
        (v * (1.0 + spread * (self.next() * 2.0 - 1.0))).clamp(lo, hi)
    }
}

/// Time, with an impact charged at half a second and time off the road charged
/// at its own duration — so the search cannot buy a lap record with contact.
fn cost(r: &agent::AgentRace) -> f32 {
    r.finished
        .then(|| {
            r.elapsed_seconds
                + f32::from(r.impacts.min(600) as u16) * 0.5
                + f32::from(r.offroad_steps.min(600) as u16) / 60.0
        })
        .unwrap_or(f32::INFINITY)
}

#[test]
#[ignore]
fn search_rails() {
    run_search(PlayProfile::Rails, DriverTuning::FAST);
}

#[test]
#[ignore]
fn search() {
    run_search(PlayProfile::Wheel, DriverTuning::FAST);
}

fn run_search(profile: PlayProfile, seed_tuning: DriverTuning) {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut best = seed_tuning;
    let mut best_cost = cost(&agent::race(sim_for(profile), &best, 60 * 60 * 20));
    println!("start {best_cost:.3} on {profile:?}");

    (0..12).for_each(|round| {
        let spread = [0.35f32, 0.18, 0.08][(round / 4).min(2)];
        (0..220).for_each(|_| {
            let mut d = best;
            d.lookahead_base = rng.jitter(d.lookahead_base, spread, 0.5, 14.0);
            d.lookahead_per_speed = rng.jitter(d.lookahead_per_speed, spread, 0.15, 0.7);
            d.traffic_horizon = rng.jitter(d.traffic_horizon, spread, 20.0, 220.0);
            d.touch_margin = rng.jitter(d.touch_margin, spread, 0.2, 3.0);
            d.edge_margin = rng.jitter(d.edge_margin, spread, 0.0, 3.0);
            d.centre_pull = rng.jitter(d.centre_pull, spread, 0.0, 0.8);
            d.urgency_falloff = rng.jitter(d.urgency_falloff, spread, 0.0, 1.2);
            d.lane_change_cost = rng.jitter(d.lane_change_cost, spread, 0.0, 0.5);
            d.boost_start_charge = rng.jitter(d.boost_start_charge, spread, 0.06, 0.9);
            d.near_miss_reward = rng.jitter(d.near_miss_reward, spread, 0.0, 40.0);
            d.steer_gain_milli =
                rng.jitter(d.steer_gain_milli as f32, spread, 3_000.0, 40_000.0) as i64;
            d.steer_damping_milli =
                rng.jitter(d.steer_damping_milli as f32, spread, 0.0, 900.0) as i64;
            let r = agent::race(sim_for(profile), &d, 60 * 60 * 20);
            let c = cost(&r);
            (c < best_cost - 0.001).then(|| {
                best_cost = c;
                best = d;
                println!(
                    "  r{round} {:.3}  time={:.2} nm={} imp={} off={} boost={}",
                    c, r.elapsed_seconds, r.near_misses, r.impacts, r.offroad_steps, r.boost_steps
                );
            });
        });
    });

    let r = agent::race(sim_for(profile), &best, 60 * 60 * 20);
    println!(
        "\nBEST cost {best_cost:.3}  time={:.2}s nm={} imp={} off={} boost={} mean={:.1}\n{best:#?}",
        r.elapsed_seconds, r.near_misses, r.impacts, r.offroad_steps, r.boost_steps, r.mean_speed
    );
}

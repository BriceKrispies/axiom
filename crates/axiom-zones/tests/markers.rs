//! Proves each zone attribute injects its greppable marker. Each function/module
//! references the marker the macro is supposed to have injected — so this only
//! compiles if injection worked, no `cargo expand` required.

use axiom_zones::{escape_hatch, hot_path, sim, strict, supervisor};

#[sim]
fn sim_fn() {
    __engine_zone_sim
}

#[hot_path]
fn hot_fn() {
    __engine_zone_hot_path
}

#[strict]
fn strict_fn() {
    __engine_zone_strict
}

#[supervisor]
fn supervisor_fn() {
    __engine_zone_supervisor
}

#[escape_hatch(reason = "documented and deliberate")]
fn escape_hatch_fn() -> &'static str {
    __engine_escape_hatch_reason
}

#[sim]
mod sim_mod {
    pub fn reads_module_marker() {
        __engine_zone_sim
    }
}

struct Stepper;

impl Stepper {
    // The engine's real sim entry points are impl methods, not free fns.
    #[sim]
    fn step(&self) {
        __engine_zone_sim
    }
}

#[test]
fn every_marker_is_injected() {
    sim_fn();
    hot_fn();
    strict_fn();
    supervisor_fn();
    sim_mod::reads_module_marker();
    Stepper.step();
    assert_eq!(escape_hatch_fn(), "documented and deliberate");
}

// Path is `modules/m/src/` — an engine module, but NOT the physics crate. Code
// here that is NOT inside a `#[sim]` zone is authoring-time math and is allowed:
// trig used once at setup is not on the replayed per-step path. The `#[sim]`
// markers are written exactly as `axiom_zones::sim` injects them, so the fixture
// needs no dependency on the markers crate.
#![allow(dead_code, non_upper_case_globals)]

// ---- NOT flagged: authoring trig outside any `#[sim]` zone ----

// The `Quat::from_axis_angle` shape — sin/cos at construction, not per step.
fn from_axis_angle(half: f32) -> (f32, f32) {
    (half.sin(), half.cos())
}

// An easing curve — `powf` at authoring time.
fn ease(t: f32) -> f32 {
    2f32.powf(-10.0 * t)
}

// A logger-shaped type with a `.log()` method must NOT be confused with the
// float `log`: its receiver isn't a float, so the lint stays silent.
struct Logger;
impl Logger {
    fn log(&self, _msg: &str) {}
}
fn uses_logger(l: &Logger) {
    l.log("tick");
}

// ---- FLAGGED: the same ops INSIDE a `#[sim]` zone ----

fn sim_step(v: f32, dt: f32, p: f32) -> f32 {
    const __engine_zone_sim: () = ();
    v.mul_add(dt, p)
}

// A `#[sim]` MODULE zones everything inside it, even an unmarked fn.
mod sim_zone {
    const __engine_zone_sim: () = ();

    pub fn wobble(theta: f32) -> f32 {
        theta.sin()
    }
}

fn main() {}

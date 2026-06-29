// Path contains `modules/axiom-physics/src/`, so this is the physics step path:
// the whole crate is the deterministic spine, and unportable float ops are
// banned here even without a `#[sim]` marker. The portable subset
// `{+,-,*,/,sqrt,min,max}` is fine.
#![allow(dead_code)]

// ---- FLAGGED: unportable float in the physics step path ----

// Fused multiply-add rounds once (a hardware FMA on some targets, a polyfill on
// others) — a different bit-pattern from `p + v * dt`.
fn integrate(p: f32, v: f32, dt: f32) -> f32 {
    v.mul_add(dt, p)
}

// `libm` transcendentals are not bit-identical across platforms.
fn swing(theta: f32) -> f32 {
    theta.sin() + theta.cos()
}

fn decay(x: f64) -> f64 {
    x.exp()
}

// ---- NOT flagged: the portable subset `{+,-,*,/,sqrt,min,max}` ----

fn portable(p: f32, v: f32, dt: f32) -> f32 {
    p + v * dt
}

fn magnitude(x: f32, y: f32) -> f32 {
    // `sqrt` is IEEE-correctly-rounded everywhere; `min`/`max` are portable too.
    (x * x + y * y).sqrt().max(0.0)
}

fn main() {}
